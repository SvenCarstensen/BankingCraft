"""Verify that rejected/timed-out notarizations cannot produce publishable DMGs."""
from contextlib import contextmanager
import importlib.util
import os
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("packaging", Path(__file__).resolve().parents[1] / "package-release.py")
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


class PackageSigningTests(unittest.TestCase):
    def test_native_data_moves_to_resources_without_changing_runtime_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            app = Path(directory) / "BankingCraft.app"
            native = app / "Contents/MacOS/native"
            binary_names = ["bin/worker", "lib/plugins/plugin.so", "lib/libexample.dylib"]
            data_names = ["lib/plugins/plugin.xml", "share/data.xml", "licenses/gmp/COPYING.LESSERv3", "minimum-macos.txt"]
            for name in binary_names + data_names:
                path = native / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(bytes.fromhex("cffaedfe") if name in binary_names else b"fixture data")
            packaging.relocate_macos_native_resources(app)
            self.assertEqual(set(packaging.macos_signing.macho_files(app)), {native / name for name in binary_names})
            for name in binary_names:
                self.assertFalse((native / name).is_symlink())
            for name in data_names:
                target = app / "Contents/Resources/native" / name
                self.assertEqual((native / name).resolve(), target.resolve())
                self.assertEqual((native / name).read_bytes(), b"fixture data")
            # Reapplying the layout must not move or replace the existing links.
            packaging.relocate_macos_native_resources(app)
            self.assertTrue((native / "share").is_symlink())

    def exercise(self, failure):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "macos").mkdir()
            (root / "macos/Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": "de.appcreation.bankingcraft"}))
            (root / "macos/BankingCraft.icns").write_bytes(b"synthetic icon")
            binary = root / "target/release/bankingcraft"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"synthetic binary")
            output = root / "dist"
            output.mkdir()
            temporary = root / "staging"
            events = []

            def run(*args):
                if len(args) > 1 and args[1] == "scripts/package-native.py":
                    native = Path(args[2]) / "native"
                    native.mkdir()
                    (native / "minimum-macos.txt").write_text("11.0")
                if args[:2] == ("hdiutil", "create"):
                    events.append("create-dmg")
                    Path(args[-1]).write_bytes(b"synthetic DMG")
                if args[0] == "spctl":
                    events.append("gatekeeper-" + ("dmg" if str(args[-1]).endswith(".dmg") else "app"))

            class Signer:
                directory = root

                def sign_app(self, path):
                    events.append("sign-app")

                def sign(self, path, **kwargs):
                    events.append("sign-dmg")

                def verify(self, path, **kwargs):
                    events.append("verify-dmg")

                def notarize(self, path, label):
                    events.append("notarize-" + label)
                    if failure == label:
                        raise SystemExit("synthetic Apple rejection")

                def staple(self, path):
                    events.append("staple-" + ("dmg" if path.suffix == ".dmg" else "app"))

            @contextmanager
            def session(mode):
                yield Signer()

            with patch.object(packaging, "ROOT", root), patch.object(packaging, "run", side_effect=run), \
                 patch.object(packaging, "resources", side_effect=lambda path: path.mkdir(parents=True)), \
                 patch.object(packaging.platform, "machine", return_value="arm64"), \
                 patch.object(packaging.subprocess, "check_output", return_value="BankingCraft:\n /usr/lib/libSystem.B.dylib (compatibility version 1)\n"), \
                 patch.object(packaging.macos_signing, "signing_session", session), \
                 patch.object(packaging.macos_signing, "validate_configuration"), \
                 patch.dict(os.environ, {"MACOS_SIGNING_MODE": "developer-id"}):
                if failure:
                    with self.assertRaisesRegex(SystemExit, "synthetic Apple rejection"):
                        packaging.macos("1.2.3", temporary, output)
                    self.assertEqual(list(output.iterdir()), [])
                else:
                    packaging.macos("1.2.3", temporary, output)
                    self.assertEqual([p.name for p in output.iterdir()], ["BankingCraft-1.2.3-macos-aarch64.dmg"])
                    self.assertEqual(events, ["sign-app", "notarize-app", "staple-app", "gatekeeper-app",
                                              "create-dmg", "sign-dmg", "verify-dmg", "notarize-dmg", "staple-dmg", "gatekeeper-dmg"])

    def test_app_rejection_leaves_no_publishable_artifact(self):
        self.exercise("app")

    def test_dmg_rejection_leaves_no_publishable_artifact(self):
        self.exercise("dmg")

    def test_dmg_contains_stapled_app_and_is_only_exported_after_acceptance(self):
        self.exercise(None)


if __name__ == "__main__":
    unittest.main()
