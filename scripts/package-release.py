#!/usr/bin/env python3
"""Package an already-built native binary. Builds run separately in GitHub Actions."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("release_version", ROOT / "scripts/release-version.py")
release_version = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release_version)
signing_spec = importlib.util.spec_from_file_location("macos_signing", ROOT / "scripts/macos-signing.py")
macos_signing = importlib.util.module_from_spec(signing_spec)
signing_spec.loader.exec_module(macos_signing)


def run(*args, **kwargs):
    subprocess.run(args, check=True, cwd=ROOT, **kwargs)


def resources(destination):
    run(sys.executable, "scripts/update-licenses.py", "--check")
    destination.mkdir(parents=True, exist_ok=True)
    for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
        shutil.copy2(ROOT / name, destination / name)
    shutil.copytree(ROOT / "licenses", destination / "licenses")
    (destination / "app/data").mkdir(parents=True)
    shutil.copy2(ROOT / "app/data/banks.tsv", destination / "app/data/banks.tsv")
    (destination / "docs").mkdir()
    shutil.copy2(ROOT / "docs/bank-compatibility.md", destination / "docs/bank-compatibility.md")
    shutil.copy2(ROOT / "docs/releases.md", destination / "docs/releases.md")
    (destination / "BUILD_INFO.json").write_text(json.dumps({
        "version": release_version.version(),
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "platform": platform.system(),
        "architecture": platform.machine(),
        "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
    }, indent=2) + "\n")


def macos(version, temporary, output):
    mode = os.environ.get("MACOS_SIGNING_MODE", "adhoc")
    macos_signing.validate_configuration(mode)
    arch = {"arm64": "aarch64", "x86_64": "x86_64"}[platform.machine()]
    app = temporary / "image/BankingCraft.app"
    executable = app / "Contents/MacOS/BankingCraft"
    executable.parent.mkdir(parents=True)
    shutil.copy2(ROOT / "target/release/bankingcraft", executable)
    run(sys.executable, "scripts/package-native.py", str(executable.parent))
    resources(app / "Contents/Resources")
    shutil.copy2(ROOT / "macos/BankingCraft.icns", app / "Contents/Resources/BankingCraft.icns")
    info = plistlib.loads((ROOT / "macos/Info.plist").read_bytes())
    minimums = [os.environ.get("MACOSX_DEPLOYMENT_TARGET", "11.0"), (executable.parent / "native/minimum-macos.txt").read_text().strip()]
    info["LSMinimumSystemVersion"] = max(minimums, key=lambda v: tuple(map(int, v.split("."))))
    (app / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
    run("plutil", "-lint", str(app / "Contents/Info.plist"))
    linked = subprocess.check_output(["otool", "-L", str(executable)], text=True)
    for line in linked.splitlines()[1:]:
        library = line.strip().split(" (", 1)[0]
        if not library.startswith(("/System/Library/", "/usr/lib/")):
            raise SystemExit(f"Non-system library would be missing from DMG: {library}")
    (temporary / "image/Applications").symlink_to("/Applications")
    dmg = temporary / f"BankingCraft-{version}-macos-{arch}.dmg"
    with macos_signing.signing_session(mode) as signer:
        if signer:
            signer.sign_app(app)
            # Exercise the bundled worker/plugins with hardened runtime and library validation.
            run(sys.executable, "scripts/test-native.py", str(executable.parent / "native/bin/bankingcraft-aqbanking"))
            archive = signer.directory / "BankingCraft.zip"
            run("ditto", "-c", "-k", "--keepParent", str(app), str(archive))
            signer.notarize(archive, "app")
            signer.staple(app)
            run("spctl", "--assess", "--type", "execute", "--verbose=2", str(app))
        else:
            print("Ad-hoc test package only; not Developer ID signed or notarized", flush=True)
            run("codesign", "--force", "--sign", "-", str(app))
        run("codesign", "--verify", "--deep", "--strict", str(app))
        run("hdiutil", "create", "-volname", f"BankingCraft {version}", "-srcfolder",
            str(temporary / "image"), "-format", "UDZO", str(dmg))
        if signer:
            signer.sign(dmg, runtime=False)
            signer.verify(dmg, runtime=False)
            signer.notarize(dmg, "dmg")
            signer.staple(dmg)
            run("spctl", "--assess", "--type", "open", "--context", "context:primary-signature",
                "--verbose=2", str(dmg))
        run("hdiutil", "verify", str(dmg))
        # Only fully verified artifacts enter dist and become eligible for upload.
        shutil.move(str(dmg), output / dmg.name)


def windows(version, temporary, output):
    payload = temporary / "BankingCraft"
    resources(payload)
    shutil.copy2(ROOT / "target/release/bankingcraft.exe", payload / "BankingCraft.exe")
    run(sys.executable, "scripts/package-native.py", str(payload))
    shutil.copy2(ROOT / "packaging/windows/BankingCraft.ico", payload / "BankingCraft.ico")
    # Rust's MSVC target uses the shared CRT. Ship its app-local runtime with both packages.
    vswhere = Path(os.environ["ProgramFiles(x86)"]) / "Microsoft Visual Studio/Installer/vswhere.exe"
    installation = subprocess.check_output([
        str(vswhere), "-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-property", "installationPath",
    ], text=True).strip()
    redist = Path(installation) / "VC/Redist/MSVC"
    candidates = sorted(redist.glob("*/x64/Microsoft.VC*.CRT"), reverse=True)
    if not candidates:
        raise SystemExit("Visual C++ app-local redistributable was not found")
    for dll in candidates[0].glob("*.dll"):
        shutil.copy2(dll, payload / dll.name)
    if not (payload / "vcruntime140.dll").is_file():
        raise SystemExit("Visual C++ runtime missing from Windows package")
    compiler = shutil.which("ISCC.exe")
    if not compiler:
        for directory in ("ProgramFiles(x86)", "ProgramFiles"):
            candidate = Path(os.environ[directory]) / "Inno Setup 6/ISCC.exe"
            if candidate.is_file():
                compiler = str(candidate)
                break
    if not compiler:
        raise SystemExit("Inno Setup 6 is required to create the Windows installer")
    run(compiler, f"/DAppVersion={version}", f"/DPayloadDir={payload}",
        f"/O{output}", str(ROOT / "packaging/windows/BankingCraft.iss"))
    archive = output / f"BankingCraft-{version}-windows-x86_64-portable.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as bundle:
        for path in sorted(payload.rglob("*")):
            if path.is_file():
                bundle.write(path, path.relative_to(temporary))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("platform", choices=("macos", "windows", "resources"))
    parser.add_argument("--destination", type=Path)
    args = parser.parse_args()
    version = release_version.version()
    if args.platform == "resources":
        if args.destination is None:
            parser.error("resources requires --destination")
        resources(args.destination.resolve())
    else:
        output = ROOT / "dist"
        output.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="bankingcraft-package-") as directory:
            {"macos": macos, "windows": windows}[args.platform](version, Path(directory), output)
