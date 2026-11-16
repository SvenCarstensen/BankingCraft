"""Runner retry trust boundaries, limits and races; no real GitHub writes."""
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("retry", Path(__file__).resolve().parents[1] / "retry-runner-failures.py")
retry = importlib.util.module_from_spec(spec)
spec.loader.exec_module(retry)
REPO = "SvenCarstensen/BankingCraft"
RUN = {"id": 10, "run_attempt": 1, "status": "completed", "conclusion": "failure",
       "path": ".github/workflows/ci.yml", "event": "push", "head_branch": "master",
       "head_sha": "current", "head_repository": {"full_name": REPO}}
JOB = {"id": 20, "status": "completed", "conclusion": "cancelled", "runner_id": 0, "steps": []}
ANNOTATIONS = [{"annotation_level": "failure", "message": retry.ALLOCATION_FAILURE}]


class RunnerRetries(unittest.TestCase):
    def test_only_current_trusted_runs_below_attempt_limit(self):
        self.assertTrue(retry.eligible_run(RUN, REPO, "current"))
        self.assertTrue(retry.eligible_run({**RUN, "run_attempt": 2}, REPO, "current"))
        for change in ({"run_attempt": 3}, {"head_sha": "old"}, {"status": "in_progress"},
                       {"event": "pull_request"}, {"head_branch": "feature"}, {"conclusion": "success"},
                       {"path": ".github/workflows/unknown.yml"}, {"head_repository": {"full_name": "other/fork"}}):
            with self.subTest(change=change):
                self.assertFalse(retry.eligible_run({**RUN, **change}, REPO, "current"))
        self.assertTrue(retry.eligible_run({**RUN, "path": ".github/workflows/release.yml",
                                           "head_branch": "v0.1.1"}, REPO, "current"))

    def test_manual_cancellation_test_failure_and_forged_message_do_not_retry(self):
        self.assertTrue(retry.allocation_failed(JOB, ANNOTATIONS))
        self.assertFalse(retry.allocation_failed(JOB, []))
        self.assertFalse(retry.allocation_failed(JOB, [{"annotation_level": "failure", "message": "The operation was canceled."}]))
        for change in ({"steps": [{"name": "Tests", "conclusion": "failure"}]}, {"runner_id": 123},
                       {"conclusion": "timed_out"}, {"conclusion": "success"}):
            with self.subTest(change=change):
                self.assertFalse(retry.allocation_failed({**JOB, **change}, ANNOTATIONS))

    def test_multiple_infrastructure_failures_retry_together_but_not_test_failures(self):
        other = {**JOB, "id": 21}
        self.assertEqual(retry.retry_path(10, [JOB, other], [JOB, other]), "actions/runs/10/rerun-failed-jobs")
        self.assertEqual(retry.retry_path(10, [JOB, other], [JOB]), "actions/jobs/20/rerun")
        self.assertIsNone(retry.retry_path(10, [other], []))

    def execute_event(self, *, current=None, master="current", annotation=ANNOTATIONS, attempt=1):
        run = {**RUN, "run_attempt": attempt}
        current = current or run
        reads = iter([run, current])
        master_reads = iter(["current", master])
        posts = []
        def api(path, method="GET"):
            if method == "POST":
                posts.append(path)
                return None
            if path.endswith("actions/runs/10"):
                return next(reads)
            if path.endswith("branches/master"):
                return {"commit": {"sha": next(master_reads)}}
            if "/jobs?" in path:
                return {"jobs": [JOB]}
            if "/annotations?" in path:
                return annotation
            self.fail("Unexpected API endpoint " + path)
        with tempfile.TemporaryDirectory() as directory:
            event = Path(directory) / "event.json"
            event.write_text(json.dumps({"action": "completed", "workflow_run": run}))
            with patch.dict(os.environ, {"GITHUB_EVENT_PATH": str(event), "GITHUB_REPOSITORY": REPO}, clear=True), \
                    patch.object(retry, "api", side_effect=api), patch.object(retry.time, "sleep") as sleep:
                retry.main()
                return posts, [c.args[0] for c in sleep.call_args_list]

    def test_real_allocation_failure_requests_one_retry_after_backoff(self):
        for attempt, delay in ((1, 60), (2, 120)):
            posts, delays = self.execute_event(attempt=attempt)
            self.assertEqual(posts, [f"repos/{REPO}/actions/runs/10/rerun-failed-jobs"])
            self.assertEqual(delays, [delay])

    def test_limit_new_master_and_manual_rerun_prevent_writes(self):
        for options in ({"attempt": 3}, {"master": "new"}, {"annotation": []},
                        {"current": {**RUN, "run_attempt": 2, "status": "queued"}}):
            with self.subTest(options=options):
                posts, _ = self.execute_event(**options)
                self.assertEqual(posts, [])


if __name__ == "__main__":
    unittest.main()
