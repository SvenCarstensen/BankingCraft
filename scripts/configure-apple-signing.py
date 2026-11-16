#!/usr/bin/env python3
"""Validate AppCreation's Apple credentials locally and store them as GitHub Actions secrets."""
import argparse
import base64
import getpass
import os
from pathlib import Path
import subprocess
import sys


def run(*args, **kwargs):
    result = subprocess.run(args, check=False, **kwargs)
    if result.returncode:
        raise SystemExit(f"{Path(args[0]).name} failed; configuration incomplete (exit {result.returncode})")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="SvenCarstensen/BankingCraft")
    parser.add_argument("--certificate", type=Path, required=True, help="Developer ID Application .p12 including private key")
    parser.add_argument("--api-key", type=Path, required=True, help="App Store Connect team API key .p8")
    parser.add_argument("--team-id", required=True)
    parser.add_argument("--key-id", required=True)
    parser.add_argument("--issuer-id", required=True)
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("Run this one-time setup on a Mac with Xcode and gh installed")
    values = {
        "APPLE_CERTIFICATE_BASE64": base64.b64encode(args.certificate.read_bytes()).decode("ascii"),
        "APPLE_API_PRIVATE_KEY": args.api_key.read_text(),
        "APPLE_API_KEY_ID": args.key_id,
        "APPLE_API_ISSUER_ID": args.issuer_id,
        "APPLE_CERTIFICATE_PASSWORD": getpass.getpass("Password for the Developer ID .p12: "),
    }
    # Reject the wrong organization, certificate type, team or notarization credentials before writing secrets.
    environment = {**os.environ, **values, "APPLE_TEAM_ID": args.team_id, "MACOS_SIGNING_MODE": "developer-id"}
    run(sys.executable, str(Path(__file__).with_name("macos-signing.py")), "check-credentials", env=environment)
    for name, value in values.items():
        run("gh", "secret", "set", name, "--repo", args.repo, input=value, text=True)
        print(f"Stored {name}", flush=True)
    run("gh", "variable", "set", "APPLE_TEAM_ID", "--repo", args.repo, "--body", args.team_id)
    print(f"Configured {args.repo}. Run the Apple signing check workflow, then a signed release dry run.")


if __name__ == "__main__":
    main()
