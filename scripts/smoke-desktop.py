#!/usr/bin/env python3
"""Ensure a packaged GUI starts and stays alive with an empty, isolated profile."""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("executable", type=Path)
parser.add_argument("--log", type=Path)
args = parser.parse_args()

with tempfile.TemporaryDirectory(prefix="bankingcraft-smoke-") as directory:
    environment = os.environ.copy()
    for variable in ("HOME", "USERPROFILE", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "APPDATA", "LOCALAPPDATA"):
        environment[variable] = directory
    executable = args.executable.resolve()
    log_path = args.log or Path(directory) / "startup.log"
    with log_path.open("w+b") as log:
        # Windows GUI executables need explicit standard handles to expose Rust panic diagnostics.
        process = subprocess.Popen([str(executable)], cwd=directory, env=environment,
                                   stdout=log, stderr=subprocess.STDOUT)
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
            log.seek(0)
            sys.stdout.write(log.read().decode("utf-8", errors="replace"))
