#!/usr/bin/env python3
"""Include checksum-verified native upstream archives in matching source releases."""
import hashlib
import json
from pathlib import Path
import sys
import urllib.request

root=Path(__file__).resolve().parents[1]
destination=Path(sys.argv[1]);destination.mkdir(parents=True,exist_ok=True)
for package in json.loads((root/"native/dependencies.json").read_text()):
    with urllib.request.urlopen(package["url"],timeout=60) as remote:
        data=remote.read(40*1024*1024+1)
    if hashlib.sha256(data).hexdigest()!=package["sha256"]: raise SystemExit("Native source checksum mismatch")
    (destination/(package["name"]+"-"+package["version"]+".tar.gz")).write_bytes(data)
