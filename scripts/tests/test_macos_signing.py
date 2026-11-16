"""Signing policy, nested-code coverage and failure behavior; no real credentials or Apple calls."""
import base64
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("signing", SCRIPTS / "macos-signing.py")
signing = importlib.util.module_from_spec(spec)
spec.loader.exec_module(signing)

TEAM = "ABC1234567"
FINGERPRINT = "A" * 40
CONFIG = {
    "APPLE_TEAM_ID": TEAM,
    "APPLE_SIGNING_NAME": "Sven Carstensen",
    "APPLE_CERTIFICATE_BASE64": base64.b64encode(b"synthetic p12 fixture").decode(),
    "APPLE_CERTIFICATE_PASSWORD": "synthetic-password-do-not-log",
    "APPLE_API_KEY_ID": "KEY1234567",
    "APPLE_API_ISSUER_ID": "12345678-1234-1234-1234-123456789abc",
    "APPLE_API_PRIVATE_KEY": "synthetic private key fixture",
}


class SigningTests(unittest.TestCase):
    def test_missing_credentials_and_invalid_mode_fail_closed(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(SystemExit, "APPLE_TEAM_ID"):
                signing.validate_configuration("developer-id")
            with self.assertRaisesRegex(SystemExit, "must be"):
                signing.validate_configuration("auto")
            signing.validate_configuration("adhoc")

    def test_tags_and_publish_cannot_fall_back_to_adhoc(self):
        for environment in ({"GITHUB_REF": "refs/tags/v1.0.0"}, {"MACOS_PUBLISHING": "true"}):
            with self.subTest(environment=environment), patch.dict(os.environ, environment, clear=True):
                with self.assertRaisesRegex(SystemExit, "require Developer ID"):
                    signing.validate_configuration("adhoc")

    def test_identifiers_validated_before_import(self):
        for name in ("APPLE_TEAM_ID", "APPLE_API_KEY_ID", "APPLE_API_ISSUER_ID"):
            with self.subTest(name=name), patch.dict(os.environ, {**CONFIG, name: "wrong value"}, clear=True):
                with self.assertRaises(SystemExit):
                    signing.validate_configuration("developer-id")

    def test_extensionless_worker_and_plugins_signed_before_app(self):
        with tempfile.TemporaryDirectory() as directory:
            app = Path(directory) / "BankingCraft.app"
            binaries = [app / "Contents/MacOS/BankingCraft", app / "Contents/MacOS/native/bin/worker",
                        app / "Contents/MacOS/native/lib/provider/plugin.so", app / "Contents/MacOS/native/lib/lib.dylib"]
            for binary in binaries:
                binary.parent.mkdir(parents=True, exist_ok=True)
                binary.write_bytes(bytes.fromhex("cffaedfe") + b"synthetic Mach-O")
            (app / "Contents/MacOS/native/lib/link.dylib").symlink_to(binaries[-1])
            (app / "Contents/Info.plist").write_text("not executable")
            signer = signing.SigningSession(Path(directory), FINGERPRINT, TEAM, "Sven Carstensen")
            with patch.object(signer, "sign") as sign, patch.object(signer, "verify") as verify, patch.object(signing, "run"):
                signer.sign_app(app)
            signed = [call.args[0] for call in sign.call_args_list]
            self.assertEqual(set(signed[:-1]), set(binaries))
            self.assertEqual(signed[-1], app)
            self.assertEqual(signed[0], binaries[2])
            self.assertEqual(verify.call_count, 5)

    def test_signature_metadata_rejects_wrong_team_or_missing_protections(self):
        valid = (f"TeamIdentifier={TEAM}\nAuthority=Developer ID Application: Sven Carstensen ({TEAM})\n"
                 "CodeDirectory flags=0x10000(runtime)\nTimestamp=Nov 16, 2026\n")
        signer = signing.SigningSession(Path("/tmp/fixture"), FINGERPRINT, TEAM, "Sven Carstensen")
        with patch.object(signing, "run", return_value=subprocess.CompletedProcess([], 0, stderr=valid)):
            signer.verify(Path("BankingCraft.app"))
        for invalid in (valid.replace(TEAM, "OTHER12345"), valid.replace("Sven Carstensen", "Other Signer"),
                        valid.replace("0x10000", "0x0"), valid.replace("Timestamp=", "Signed Time=")):
            with self.subTest(details=invalid), patch.object(signing, "run", return_value=subprocess.CompletedProcess([], 0, stderr=invalid)):
                with self.assertRaises(SystemExit):
                    signer.verify(Path("BankingCraft.app"))

    def test_notary_rejections_timeouts_and_malformed_results_stop_packaging(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {}, clear=True):
            signer = signing.SigningSession(Path(directory), FINGERPRINT, TEAM, "Sven Carstensen")
            cases = [(0, {"id": "submission", "status": "Accepted"}, True),
                     (0, {"id": "submission", "status": "Invalid"}, False),
                     (1, {"id": "submission", "status": "In Progress"}, False),
                     (0, {"status": "Accepted"}, False), (1, None, False)]
            for code, data, accepted in cases:
                with self.subTest(code=code, data=data):
                    def notary(args, **kwargs):
                        if args[2] == "submit":
                            kwargs["stdout"].write(json.dumps(data) if data else "connection failed")
                            return subprocess.CompletedProcess(args, code)
                        return subprocess.CompletedProcess(args, 0)
                    with patch.object(signing.subprocess, "run", side_effect=notary):
                        if accepted:
                            signer.notarize(Path("fixture.zip"), "app")
                        else:
                            with self.assertRaises(SystemExit):
                                signer.notarize(Path("fixture.zip"), "app")

    def test_command_failure_does_not_print_secret_arguments(self):
        with patch.object(signing.subprocess, "run", return_value=subprocess.CompletedProcess([], 1)):
            with self.assertRaises(SystemExit) as raised:
                signing.run("security", "import", "-P", CONFIG["APPLE_CERTIFICATE_PASSWORD"])
        self.assertNotIn(CONFIG["APPLE_CERTIFICATE_PASSWORD"], str(raised.exception))

    def test_temporary_keychain_cleaned_on_success_wrong_identity_and_exception(self):
        identities = [f'Developer ID Application: Sven Carstensen ({TEAM})',
                      f'Apple Distribution: Sven Carstensen ({TEAM})',
                      f'Developer ID Application: Other Signer ({TEAM})',
                      'Developer ID Application: Sven Carstensen (OTHER12345)']
        for identity in identities:
            for fail_inside in (False, True):
                with self.subTest(identity=identity, fail_inside=fail_inside), tempfile.TemporaryDirectory() as parent:
                    deleted = []
                    def security(args, **kwargs):
                        if args[:2] == ("security", "create-keychain"):
                            Path(args[-1]).touch()
                        if args[:2] == ["security", "delete-keychain"]:
                            deleted.append(Path(args[-1]))
                        stdout = f'  1) {FINGERPRINT} "{identity}"\n'
                        return subprocess.CompletedProcess(args, 0, stdout=stdout)
                    with patch.dict(os.environ, {**CONFIG, "RUNNER_TEMP": parent}, clear=True), patch.object(signing.subprocess, "run", side_effect=security):
                        def execute():
                            with signing.signing_session("developer-id") as signer:
                                self.assertEqual(signer.identity, FINGERPRINT)
                                self.assertFalse((signer.directory / "certificate.p12").exists())
                                self.assertFalse((signer.directory / "AuthKey.p8").exists())
                                if fail_inside:
                                    raise RuntimeError("synthetic packaging failure")
                        if identity != identities[0]:
                            with self.assertRaisesRegex(SystemExit, "Expected exactly one"):
                                execute()
                        elif fail_inside:
                            with self.assertRaisesRegex(RuntimeError, "synthetic"):
                                execute()
                        else:
                            execute()
                    self.assertEqual(len(deleted), 1)
                    self.assertEqual(list(Path(parent).iterdir()), [])


if __name__ == "__main__":
    unittest.main()
