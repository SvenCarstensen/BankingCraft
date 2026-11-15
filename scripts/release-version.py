#!/usr/bin/env python3
"""Validate release metadata without compiling the application."""
import os
from pathlib import Path
import plistlib
import re
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def version():
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    value = manifest["workspace"]["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", value):
        raise SystemExit(f"Expected a numeric release version, got {value!r}")
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    names = {"bankingcraft", "bankingcraft-i18n", "bankingcraft-i18n-macros", "bankingcraft-models", "bankingcraft-aqbanking"}
    packages = {p["name"]: p["version"] for p in lock["package"] if p["name"] in names}
    if set(packages) != names or set(packages.values()) != {value}:
        raise SystemExit("Cargo.lock workspace versions do not match Cargo.toml")
    plist = plistlib.loads((ROOT / "macos/Info.plist").read_bytes())
    if any(plist[key] != value for key in ("CFBundleShortVersionString", "CFBundleVersion")):
        raise SystemExit("macOS bundle versions do not match Cargo.toml")
    if os.environ.get("GITHUB_REF_TYPE") == "tag":
        if os.environ.get("GITHUB_REF_NAME") != f"v{value}":
            raise SystemExit("Release tag does not match Cargo.toml")
    return value


if __name__ == "__main__":
    print(version())
