#!/usr/bin/env python3
"""Developer ID signing and notarization for the GitHub release runner."""
import argparse
import base64
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import subprocess
import tempfile
import time


REQUIRED = (
    "APPLE_TEAM_ID", "APPLE_SIGNING_NAME", "APPLE_CERTIFICATE_BASE64", "APPLE_CERTIFICATE_PASSWORD",
    "APPLE_API_KEY_ID", "APPLE_API_ISSUER_ID", "APPLE_API_PRIVATE_KEY",
)
MACHO_MAGIC = {
    bytes.fromhex(value) for value in (
        "feedface", "cefaedfe", "feedfacf", "cffaedfe",
        "cafebabe", "bebafeca", "cafebabf", "bfbafeca",
    )
}
APPLE_RETRY_DELAYS = (10, 30, 60)
NOTARY_WAIT_SECONDS = 30 * 60


def transient_apple_error(message):
    """Retry transport failures, never credentials, signatures or Apple rejections."""
    if re.search(r"(?:status(?:Code)?[\s:=]*(?:Optional\()?|HTTP\s+)(?:401|403)\b|"
                 r"invalid credentials|authentication failed|signature.*invalid|status.*(?:Invalid|Rejected)",
                 message, re.IGNORECASE):
        return False
    return bool(re.search(
        r"NSURLErrorDomain\s+Code=-(?:1001|1003|1004|1005|1006|1009)\b|"
        r"(?:status(?:Code)?[\s:=]*(?:Optional\()?|HTTP\s+)(?:429|5\d\d)\b|"
        r"request timed out|network connection was lost|could not connect to the server|"
        r"could not resolve host|connection reset by peer|timestamp service is not available",
        message, re.IGNORECASE))


def apple_request(*args, retry=True, timeout=120, deadline=None):
    """Bounded retries for idempotent Apple requests; never log argv or credentials."""
    result = subprocess.CompletedProcess(args, 124, stdout="", stderr="Apple request timed out")
    for attempt in range(len(APPLE_RETRY_DELAYS) + 1 if retry else 1):
        remaining = timeout if deadline is None else min(timeout, deadline - time.monotonic())
        if remaining <= 0:
            return result
        try:
            result = subprocess.run(args, capture_output=True, text=True, check=False, timeout=remaining)
        except subprocess.TimeoutExpired as error:
            # Keep partial JSON (including a possible submission ID), without printing argv.
            def decoded(value):
                return value.decode(errors="replace") if isinstance(value, bytes) else (value or "")
            result = subprocess.CompletedProcess(args, 124, stdout=decoded(error.stdout),
                                                 stderr=decoded(error.stderr) + "\nApple request timed out")
        if not result.returncode or not retry or not transient_apple_error((result.stdout or "") + (result.stderr or "")):
            return result
        if attempt == len(APPLE_RETRY_DELAYS):
            return result
        delay = APPLE_RETRY_DELAYS[attempt]
        if deadline is not None and time.monotonic() + delay >= deadline:
            return result
        print(f"Temporary Apple network failure; retry {attempt + 2}/4 in {delay}s", flush=True)
        time.sleep(delay)
    return result


def apple_run(*args):
    result = apple_request(*args)
    if result.returncode:
        raise SystemExit(f"Apple {Path(args[0]).name} request failed (exit {result.returncode}); signing stopped")
    return result


def run(*args, **kwargs):
    # CalledProcessError includes argv, which can contain a certificate password.
    result = subprocess.run(args, check=False, **kwargs)
    if result.returncode:
        raise SystemExit(f"{Path(args[0]).name} failed (exit {result.returncode}); signing stopped")
    return result


def validate_configuration(mode):
    if mode not in ("adhoc", "developer-id"):
        raise SystemExit("MACOS_SIGNING_MODE must be adhoc or developer-id")
    publishing = os.environ.get("MACOS_PUBLISHING", "false") == "true"
    if mode == "adhoc":
        if publishing or os.environ.get("GITHUB_REF", "").startswith("refs/tags/v"):
            raise SystemExit("Published macOS releases require Developer ID signing and notarization")
        return
    missing = [key for key in REQUIRED if not os.environ.get(key, "").strip()]
    if missing:
        raise SystemExit("Missing Apple signing configuration: " + ", ".join(missing))
    if not re.fullmatch(r"[A-Z0-9]{10}", os.environ["APPLE_TEAM_ID"]):
        raise SystemExit("APPLE_TEAM_ID must be the authorized 10-character Apple team ID")
    if any(character in os.environ["APPLE_SIGNING_NAME"] for character in "\r\n\"()"):
        raise SystemExit("APPLE_SIGNING_NAME must be the certificate holder's name without its team suffix")
    if not re.fullmatch(r"[A-Za-z0-9]+", os.environ["APPLE_API_KEY_ID"]):
        raise SystemExit("Invalid APPLE_API_KEY_ID")
    if not re.fullmatch(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}", os.environ["APPLE_API_ISSUER_ID"]):
        raise SystemExit("Invalid APPLE_API_ISSUER_ID (expected a UUID)")


def macho_files(app):
    result = []
    for path in app.rglob("*"):
        if path.is_symlink() or not path.is_file():
            continue
        with path.open("rb") as stream:
            if stream.read(4) in MACHO_MAGIC:
                result.append(path)
    # Sign plugins/libraries/helpers before the enclosing application bundle.
    return sorted(result, key=lambda p: (-len(p.parts), str(p)))


class SigningSession:
    def __init__(self, directory, identity, team, name):
        self.directory = directory
        self.keychain = directory / "signing.keychain-db"
        self.identity = identity
        self.team = team
        self.authority = f"Developer ID Application: {name} ({team})"

    def sign(self, path, *, runtime=True):
        options = ("--options", "runtime") if runtime else ()
        apple_run("codesign", "--force", "--sign", self.identity, "--keychain", str(self.keychain),
            *options, "--timestamp", str(path))

    def verify(self, path, *, runtime=True):
        run("codesign", "--verify", "--strict", "--verbose=2", str(path))
        details = run("codesign", "--display", "--verbose=4", str(path),
                      capture_output=True, text=True).stderr
        if f"TeamIdentifier={self.team}" not in details.splitlines():
            raise SystemExit(f"Unexpected signing team: {path.name}")
        if f"Authority={self.authority}" not in details.splitlines():
            raise SystemExit(f"Unexpected Developer ID authority: {path.name}")
        if not any(line.startswith("Timestamp=") for line in details.splitlines()):
            raise SystemExit(f"Secure signing timestamp missing: {path.name}")
        flags = re.search(r"^CodeDirectory .*flags=(0x[0-9a-fA-F]+)", details, re.MULTILINE)
        if runtime and (not flags or not int(flags.group(1), 16) & 0x10000):
            raise SystemExit(f"Hardened runtime missing: {path.name}")

    def sign_app(self, app):
        binaries = macho_files(app)
        if not binaries:
            raise SystemExit("No Mach-O code found in the application")
        for path in binaries:
            self.sign(path)
        self.sign(app)
        for path in binaries:
            self.verify(path)
        self.verify(app)
        run("codesign", "--verify", "--deep", "--strict", str(app))

    def notarize(self, path, label):
        logs = Path(os.environ.get("MACOS_NOTARY_LOG_DIR", self.directory / "logs"))
        logs.mkdir(parents=True, exist_ok=True)
        result_path = logs / f"{label}-submission.json"
        authentication = ("--keychain-profile", "bankingcraft-notary", "--keychain", str(self.keychain))
        # Upload once, then poll the same ID. An ambiguous upload failure must not
        # create duplicate submissions; diagnostics allow an operator to reconcile it.
        result = apple_request("xcrun", "notarytool", "submit", str(path), *authentication,
                               "--output-format", "json", retry=False, timeout=600)
        result_path.write_text(result.stdout or "")
        try:
            submission = json.loads(result_path.read_text())
        except (ValueError, OSError):
            raise SystemExit(f"Notarization returned no valid result; inspect {result_path}") from None
        if not isinstance(submission, dict):
            raise SystemExit(f"Notarization returned no valid result; inspect {result_path}")
        submission_id = submission.get("id")
        if not isinstance(submission_id, str) or not submission_id:
            raise SystemExit(f"No notarization submission ID; inspect {result_path} before retrying the upload")
        if result.returncode and submission.get("status") == "Accepted":
            # An interrupted command is not itself proof of acceptance.
            submission["status"] = "In Progress"
        print(f"Notarization submitted: {label} ({submission_id})", flush=True)
        deadline = time.monotonic() + NOTARY_WAIT_SECONDS
        while submission.get("status") not in ("Accepted", "Invalid", "Rejected"):
            if time.monotonic() >= deadline:
                raise SystemExit(f"Notarization timed out; submission {submission_id}; see {result_path}")
            result = apple_request("xcrun", "notarytool", "info", submission_id, *authentication,
                                   "--output-format", "json", deadline=deadline)
            if result.returncode:
                raise SystemExit(f"Notarization status unavailable; submission {submission_id}; see {result_path}")
            try:
                submission = json.loads(result.stdout)
            except ValueError:
                raise SystemExit(f"Invalid notarization status; submission {submission_id}") from None
            if not isinstance(submission, dict) or submission.get("id") != submission_id:
                raise SystemExit(f"Unexpected notarization status ID; submission {submission_id}")
            (logs / f"{label}-status.json").write_text(result.stdout)
            if submission.get("status") == "In Progress":
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise SystemExit(f"Notarization timed out; submission {submission_id}; see {result_path}")
                time.sleep(min(15, remaining))
            elif submission.get("status") not in ("Accepted", "Invalid", "Rejected"):
                raise SystemExit(f"Unknown notarization status; submission {submission_id}")
        # Keep Apple's diagnostics outside dist so they cannot become release assets.
        diagnostic = apple_request("xcrun", "notarytool", "log", submission_id, *authentication,
                                   str(logs / f"{label}-log.json"))
        if diagnostic.returncode:
            print(f"Warning: notarization log unavailable for {submission_id}", flush=True)
        if submission.get("status") != "Accepted":
            raise SystemExit(f"Notarization not accepted ({submission.get('status', 'unknown')}); see {result_path}")
        print(f"Notarization accepted: {label} ({submission_id})", flush=True)

    def staple(self, path):
        apple_run("xcrun", "stapler", "staple", str(path))
        run("xcrun", "stapler", "validate", str(path))


@contextmanager
def signing_session(mode):
    validate_configuration(mode)
    if mode == "adhoc":
        yield None
        return
    parent = os.environ.get("RUNNER_TEMP") or tempfile.gettempdir()
    with tempfile.TemporaryDirectory(prefix="bankingcraft-signing-", dir=parent) as temp:
        directory = Path(temp)
        keychain = directory / "signing.keychain-db"
        password = secrets.token_urlsafe(32)
        search_list = shlex.split(run("security", "list-keychains", "-d", "user",
                                     capture_output=True, text=True).stdout)
        try:
            certificate = directory / "certificate.p12"
            private_key = directory / "AuthKey.p8"
            try:
                certificate.write_bytes(base64.b64decode(
                    "".join(os.environ["APPLE_CERTIFICATE_BASE64"].split()), validate=True))
            except ValueError:
                raise SystemExit("APPLE_CERTIFICATE_BASE64 is not valid base64") from None
            private_key.write_text(os.environ["APPLE_API_PRIVATE_KEY"])
            certificate.chmod(0o600)
            private_key.chmod(0o600)
            run("security", "create-keychain", "-p", password, str(keychain))
            run("security", "set-keychain-settings", "-lut", "7200", str(keychain))
            run("security", "unlock-keychain", "-p", password, str(keychain))
            # Hosted macOS runners need the keychain in the search list even
            # with codesign --keychain. Preserve every existing entry.
            run("security", "list-keychains", "-d", "user", "-s", str(keychain), *search_list)
            run("security", "import", str(certificate), "-k", str(keychain),
                "-P", os.environ["APPLE_CERTIFICATE_PASSWORD"], "-t", "cert", "-f", "pkcs12",
                "-T", "/usr/bin/codesign", "-T", "/usr/bin/security", stdout=subprocess.DEVNULL)
            run("security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:",
                "-s", "-k", password, str(keychain), stdout=subprocess.DEVNULL)
            identities = run("security", "find-identity", "-v", "-p", "codesigning", str(keychain),
                             capture_output=True, text=True).stdout
            team = os.environ["APPLE_TEAM_ID"]
            name = os.environ["APPLE_SIGNING_NAME"]
            expected = f"Developer ID Application: {name} ({team})"
            matches = re.findall(r'\b([0-9A-F]{40}) "' + re.escape(expected) + '"', identities)
            if len(matches) != 1:
                raise SystemExit("Expected exactly one valid Developer ID Application certificate "
                                 "with private key for APPLE_SIGNING_NAME and APPLE_TEAM_ID")
            apple_run("xcrun", "notarytool", "store-credentials", "bankingcraft-notary",
                "--key", str(private_key), "--key-id", os.environ["APPLE_API_KEY_ID"],
                "--issuer", os.environ["APPLE_API_ISSUER_ID"], "--keychain", str(keychain))
            certificate.unlink()
            private_key.unlink()
            yield SigningSession(directory, matches[0], team, name)
        finally:
            result = subprocess.run(["security", "list-keychains", "-d", "user", "-s", *search_list], check=False)
            if result.returncode:
                print("Warning: restoring the keychain search list failed", flush=True)
            if keychain.exists():
                result = subprocess.run(["security", "delete-keychain", str(keychain)], check=False)
                if result.returncode:
                    print("Warning: keychain deletion failed; temporary directory cleanup follows", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("preflight", "check-credentials"))
    args = parser.parse_args()
    selected = os.environ.get("MACOS_SIGNING_MODE", "developer-id")
    validate_configuration(selected)
    if args.command == "check-credentials":
        if selected != "developer-id":
            parser.error("check-credentials requires MACOS_SIGNING_MODE=developer-id")
        with signing_session(selected) as signer:
            source = signer.directory / "probe.c"
            binary = signer.directory / "probe"
            source.write_text("int main(void) { return 0; }\n")
            run("xcrun", "clang", str(source), "-o", str(binary))
            signer.sign(binary)
            signer.verify(binary)
            print("Developer ID signing probe and notarization credentials validated")
    else:
        print(f"macOS signing configuration: {selected}")
