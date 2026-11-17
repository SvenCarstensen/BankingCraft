#!/usr/bin/env python3
"""Offline IPC/isolation regressions. Uses synthetic configuration; no bank I/O."""
import argparse
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import random

ROOT = Path(__file__).resolve().parents[1]

def encode(fields):
    result = bytearray()
    for key, value in fields.items():
        key = key.encode()
        if isinstance(value, str):
            value = value.encode()
        result += struct.pack(">H", len(key)) + key + struct.pack(">I", len(value)) + value
    return struct.pack(">I", len(result)) + result

def decode(data):
    frames = []
    while data:
        size, = struct.unpack(">I", data[:4])
        frame, data = data[4:4+size], data[4+size:]
        assert len(frame) == size and size <= 32*1024*1024
        fields = {}
        while frame:
            n, = struct.unpack(">H", frame[:2])
            key = frame[2:2+n].decode()
            frame = frame[2+n:]
            n, = struct.unpack(">I", frame[:4])
            assert len(frame) >= 4+n and key not in fields
            fields[key], frame = frame[4:4+n], frame[4+n:]
        frames.append(fields)
    return frames

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("helper", nargs="?", type=Path)
    args = parser.parse_args()
    helper = args.helper or Path((ROOT / ".native/helper-path").read_text().strip())
    with tempfile.TemporaryDirectory(prefix="bankingcraft-native-test-") as temporary:
        home = Path(temporary)
        blocker = home / "blocked"
        blocker.touch(mode=0o600)
        legacy = home / ".aqbanking"
        legacy.mkdir()
        (legacy / "settings.conf").write_text("DO_NOT_IMPORT_SYNTHETIC_MARKER\n")
        env = os.environ.copy()
        env.update(HOME=str(home), USERPROFILE=str(home), AQBANKING_HOME=str(legacy), GWEN_LOGLEVEL="debug", AQHBCI_LOGLEVEL="debug")
        initial = {str(p.relative_to(home)):p.read_bytes() for p in home.rglob("*") if p.is_file()}
        request = dict(version="1", op="probe", data_path=str(blocker), state=b"", blz="12345678",
                       user="synthetic-user", customer="synthetic-customer", medium="pintan", record="1",
                       url="https://bank.invalid", product="SYNTHETICREGISTRATION0001", product_version="1")
        def invoke(raw):
            # Match the Rust launcher: constructors may log before C main can
            # redirect descriptors, so stderr is discarded at process creation.
            return subprocess.run([str(helper)], input=raw, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, timeout=10)
        for op in ["probe", "configure", "configure", "probe"]:
            request["op"] = op
            result = invoke(encode(request))
            assert result.returncode == 0 and not result.stderr, (op, result.returncode)
            frames = decode(result.stdout)
            assert frames and frames[-1]["event"] == b"done", frames
            done = frames[-1]
            assert done["code"] == b"0", {k:v for k,v in done.items() if k!="state"}
            assert b"DO_NOT_IMPORT" not in done["state"]
            assert request["product"].encode() not in done["state"]
            if op == "configure":
                assert done["state"].count(b"synthetic-user") == 1
            assert "stage" in done
            request["state"] = done["state"]
        final = {str(p.relative_to(home)):p.read_bytes() for p in home.rglob("*") if p.is_file()}
        assert initial == final, "native code wrote outside the encrypted snapshot"
        for product in ["", "X"*24, "X"*26, "X"*24+"+", "X"*24+" ", "X"*24+"\0", "X"*23+"Ü"]:
            result=invoke(encode(dict(request, product=product)))
            assert result.returncode==71 and not result.stdout, "invalid HKVVB registration accepted"
        for version in ["", "123456", "1+2", "1\0"]:
            result=invoke(encode(dict(request, product_version=version)))
            assert result.returncode==71 and not result.stdout, "invalid HKVVB version accepted"
        good = encode(dict(version="1"))
        malformed = [good[:i] for i in range(len(good))]
        malformed += [struct.pack(">I", 0xffffffff), encode(dict(version=b"1\0"))]
        duplicate = good[4:]+good[4:]
        malformed.append(struct.pack(">I",len(duplicate))+duplicate)
        rng=random.Random(20261115)
        for _ in range(256):
            data=rng.randbytes(rng.randrange(1,512))
            # Invalid map key sizes are rejected before any banking operation.
            data=b"\xff\xff"+data
            malformed.append(struct.pack(">I",len(data))+data)
        for raw in malformed:
            result = invoke(raw)
            assert result.returncode == 71 and not result.stdout and not result.stderr
    print("PASS: offline initialization, state roundtrip, legacy-import isolation, HKVVB identity bounds, no disk writes, malformed IPC")

if __name__ == "__main__":
    main()
