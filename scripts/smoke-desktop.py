#!/usr/bin/env python3
"""Ensure a packaged GUI starts and stays alive with an empty, isolated profile."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile

with tempfile.TemporaryDirectory(prefix="bankingcraft-smoke-") as directory:
    environment = os.environ.copy()
    for variable in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "APPDATA", "LOCALAPPDATA"):
        environment[variable] = directory
    executable = Path(sys.argv[1]).resolve()
    process = subprocess.Popen([str(executable)], cwd=directory, env=environment)
    try:
        try:
            code = process.wait(timeout=12)
        except subprocess.TimeoutExpired:
            print("Packaged application remained running for 12 seconds.")
        else:
            raise SystemExit(f"Packaged application exited unexpectedly with code {code}")
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
