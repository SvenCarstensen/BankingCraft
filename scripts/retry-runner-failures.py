#!/usr/bin/env python3
"""Retry proven runner allocation failures, using only trusted default-branch code."""
import json
import os
from pathlib import Path
import re
import subprocess
import time

WORKFLOWS = {
    ".github/workflows/ci.yml", ".github/workflows/dependencies.yml",
    ".github/workflows/apple-signing-check.yml", ".github/workflows/release.yml",
}
MAX_ATTEMPTS = 3  # Original run plus at most two automated retries (manual retries count too).
ALLOCATION_FAILURE = "The job was not acquired by Runner of type hosted even after multiple attempts"


def api(path, method="GET"):
    result = subprocess.run(["gh", "api", "--method", method, path],
                            capture_output=True, text=True, check=False)
    if result.returncode:
        # Do not print command arguments, response bodies, tokens or event payloads.
        raise SystemExit(f"GitHub API {method} failed (exit {result.returncode}); no further retry requested")
    return json.loads(result.stdout) if result.stdout.strip() else None


def pages(path, key=None):
    separator = "&" if "?" in path else "?"
    for page in range(1, 101):
        response = api(f"{path}{separator}per_page=100&page={page}")
        items = response[key] if key else response
        yield from items
        if len(items) < 100:
            return
    raise SystemExit("GitHub pagination limit reached; refusing an incomplete retry decision")


def eligible_run(run, repository, master_sha):
    branch = run.get("head_branch", "")
    version_tag = (run.get("path") == ".github/workflows/release.yml"
                   and run.get("event") == "push"
                   and re.fullmatch(r"v\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?", branch))
    return bool(
        run.get("status") == "completed"
        and run.get("conclusion") in ("failure", "cancelled")
        and run.get("path") in WORKFLOWS
        and run.get("event") in ("push", "workflow_dispatch", "schedule")
        and run.get("head_repository", {}).get("full_name") == repository
        and (branch == "master" or version_tag)
        and run.get("head_sha") == master_sha
        and 1 <= run.get("run_attempt", 0) < MAX_ATTEMPTS
    )


def allocation_failed(job, annotations):
    # Runner-generated allocation errors occur before any step or runner exists.
    # A workflow command printed by tests cannot satisfy these conditions.
    return (job.get("status") == "completed"
            and job.get("conclusion") in ("failure", "cancelled")
            and not job.get("steps") and not job.get("runner_id")
            and any(a.get("annotation_level") == "failure"
                    and a.get("message", "").strip().rstrip(".") == ALLOCATION_FAILURE
                    for a in annotations))


def retry_path(run_id, failed_jobs, candidates):
    if not candidates:
        return None
    if len(candidates) == len(failed_jobs):
        return f"actions/runs/{run_id}/rerun-failed-jobs"
    # Keep unrelated application failures unchanged; GitHub also reruns dependents.
    return f"actions/jobs/{candidates[0]['id']}/rerun"


def report(message):
    print(message, flush=True)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as output:
            output.write(message + "\n")


def main():
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    repository = os.environ["GITHUB_REPOSITORY"]
    source = event.get("workflow_run", {})
    if event.get("action") != "completed" or not isinstance(source.get("id"), int):
        report("No completed source workflow; nothing to retry.")
        return
    base = f"repos/{repository}/"
    run = api(f"{base}actions/runs/{source['id']}")
    master_sha = api(f"{base}branches/master")["commit"]["sha"]
    if run.get("run_attempt") != source.get("run_attempt") or not eligible_run(run, repository, master_sha):
        report("No retry: successful, active, superseded, untrusted or retry limit reached.")
        return
    failed = [j for j in pages(f"{base}actions/runs/{run['id']}/jobs?filter=latest", "jobs")
              if j.get("conclusion") in ("failure", "cancelled", "timed_out", "action_required")]
    candidates = []
    for job in failed:
        if job.get("steps") or job.get("runner_id"):
            continue
        # Never follow a URL from the triggering run with a privileged token.
        annotations = list(pages(f"{base}check-runs/{job['id']}/annotations"))
        if allocation_failed(job, annotations):
            candidates.append(job)
    path = retry_path(run["id"], failed, candidates)
    if not path:
        report("No retry: no confirmed hosted-runner allocation failure before job startup.")
        return
    delay = 60 * run["run_attempt"]
    report(f"Confirmed runner allocation failure; waiting {delay}s before attempt {run['run_attempt'] + 1}/{MAX_ATTEMPTS}.")
    time.sleep(delay)
    # A manual rerun or a newer master push during the delay must win.
    current = api(f"{base}actions/runs/{run['id']}")
    master_sha = api(f"{base}branches/master")["commit"]["sha"]
    if current.get("run_attempt") != run["run_attempt"] or not eligible_run(current, repository, master_sha):
        report("Retry cancelled: run state or master changed during the delay.")
        return
    api(base + path, "POST")  # Never replay a POST after an ambiguous API failure.
    report(f"Requested attempt {run['run_attempt'] + 1}/{MAX_ATTEMPTS} for confirmed runner allocation failure(s).")


if __name__ == "__main__":
    main()
