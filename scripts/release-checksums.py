#!/usr/bin/env python3
"""Fail closed on missing/extra release assets and write SHA256SUMS.txt."""
import hashlib
import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("release_version", ROOT / "scripts/release-version.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
version = module.version()
expected = {
    f"BankingCraft-{version}-linux-x86_64.AppImage",
    f"BankingCraft-{version}-windows-x86_64-setup.exe",
    f"BankingCraft-{version}-windows-x86_64-portable.zip",
    f"BankingCraft-{version}-macos-aarch64.dmg",
    f"BankingCraft-{version}-source.tar.gz",
}
dist = ROOT / "dist"
actual = {p.name for p in dist.iterdir() if p.name != "SHA256SUMS.txt"}
if actual != expected:
    raise SystemExit(f"Release assets differ: missing={expected - actual}, extra={actual - expected}")
lines = []
for name in sorted(expected):
    path = dist / name
    if path.stat().st_size == 0:
        raise SystemExit(f"Empty release asset: {name}")
    with path.open("rb") as file:
        digest = hashlib.file_digest(file, "sha256").hexdigest()
    lines.append(f"{digest}  {name}\n")
(dist / "SHA256SUMS.txt").write_text("".join(lines))
print("Verified all five release assets and wrote SHA256SUMS.txt")
