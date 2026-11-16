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
import subprocess
import tempfile


REQUIRED = (
    "APPLE_TEAM_ID", "APPLE_CERTIFICATE_BASE64", "APPLE_CERTIFICATE_PASSWORD",
    "APPLE_API_KEY_ID", "APPLE_API_ISSUER_ID", "APPLE_API_PRIVATE_KEY",
)
MACHO_MAGIC = {
    bytes.fromhex(value) for value in (
        "feedface", "cefaedfe", "feedfacf", "cffaedfe",
        "cafebabe", "bebafeca", "cafebabf", "bfbafeca",
    )
}


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
        raise SystemExit("APPLE_TEAM_ID must be the 10-character AppCreation GmbH team ID")
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
    def __init__(self, directory, identity, team):
        self.directory = directory
        self.keychain = directory / "signing.keychain-db"
        self.identity = identity
        self.team = team

    def sign(self, path, *, runtime=True):
        options = ("--options", "runtime") if runtime else ()
        run("codesign", "--force", "--sign", self.identity, "--keychain", str(self.keychain),
            *options, "--timestamp", str(path))

    def verify(self, path, *, runtime=True):
        run("codesign", "--verify", "--strict", "--verbose=2", str(path))
        details = run("codesign", "--display", "--verbose=4", str(path),
                      capture_output=True, text=True).stderr
        if f"TeamIdentifier={self.team}" not in details.splitlines():
            raise SystemExit(f"Unexpected signing team: {path.name}")
        if f"Authority=Developer ID Application: AppCreation GmbH ({self.team})" not in details.splitlines():
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
        with result_path.open("w") as output:
            result = subprocess.run([
                "xcrun", "notarytool", "submit", str(path), *authentication,
                "--wait", "--timeout", "30m", "--output-format", "json",
            ], stdout=output, check=False)
        try:
            submission = json.loads(result_path.read_text())
        except (ValueError, OSError):
            raise SystemExit(f"Notarization returned no valid result; inspect {result_path}") from None
        if not isinstance(submission, dict):
            raise SystemExit(f"Notarization returned no valid result; inspect {result_path}")
        submission_id = submission.get("id")
        if submission_id:
            # Keep Apple's diagnostics outside dist so they cannot become release assets.
            subprocess.run([
                "xcrun", "notarytool", "log", submission_id, *authentication,
                str(logs / f"{label}-log.json"),
            ], check=False)
        if result.returncode or submission.get("status") != "Accepted" or not submission_id:
            raise SystemExit(f"Notarization not accepted ({submission.get('status', 'unknown')}); see {result_path}")
        print(f"Notarization accepted: {label} ({submission_id})", flush=True)

    def staple(self, path):
        run("xcrun", "stapler", "staple", str(path))
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
            run("security", "import", str(certificate), "-k", str(keychain),
                "-P", os.environ["APPLE_CERTIFICATE_PASSWORD"], "-t", "cert", "-f", "pkcs12",
                "-T", "/usr/bin/codesign", "-T", "/usr/bin/security", stdout=subprocess.DEVNULL)
            run("security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:",
                "-s", "-k", password, str(keychain), stdout=subprocess.DEVNULL)
            identities = run("security", "find-identity", "-v", "-p", "codesigning", str(keychain),
                             capture_output=True, text=True).stdout
            team = os.environ["APPLE_TEAM_ID"]
            expected = f"Developer ID Application: AppCreation GmbH ({team})"
            matches = re.findall(r'\b([0-9A-F]{40}) "' + re.escape(expected) + '"', identities)
            if len(matches) != 1:
                raise SystemExit("Expected exactly one valid Developer ID Application certificate "
                                 "with private key for AppCreation GmbH and APPLE_TEAM_ID")
            run("xcrun", "notarytool", "store-credentials", "bankingcraft-notary",
                "--key", str(private_key), "--key-id", os.environ["APPLE_API_KEY_ID"],
                "--issuer", os.environ["APPLE_API_ISSUER_ID"], "--keychain", str(keychain))
            certificate.unlink()
            private_key.unlink()
            yield SigningSession(directory, matches[0], team)
        finally:
            if keychain.exists():
                # The dedicated keychain is never added to the user's search list.
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
        with signing_session(selected):
            print("Apple signing certificate and notarization credentials validated")
    else:
        print(f"macOS signing configuration: {selected}")
