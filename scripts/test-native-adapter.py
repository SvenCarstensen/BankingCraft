#!/usr/bin/env python3
"""Compile the C adapter with sanitizers; test real VoP dialogs offline."""
import importlib.util
import argparse
import os
import re
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location("native_test",ROOT/"scripts/test-native.py")
wire=importlib.util.module_from_spec(spec);spec.loader.exec_module(wire)
prefix=Path((ROOT/".native/install-prefix").read_text().strip())
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("--helper",type=Path,default=prefix/"bin/bankingcraft-aqbanking")
helper=parser.parse_args().helper
source=prefix.parent/"aqbanking-6.9.2/src/libs"
env=os.environ.copy();env["PKG_CONFIG_PATH"]=str(prefix/"lib/pkgconfig")
flags=shlex.split(subprocess.check_output(["pkg-config","--cflags","--libs","aqbanking","gwenhywfar","libchipcard-client"],env=env,text=True))
if sys.platform.startswith("linux"):
    flags += ["-Wl,-rpath,$ORIGIN/../lib"]
binary=prefix/"bin/bankingcraft-native-tests"
subprocess.run([env.get("CC","cc"),"-std=c11","-g","-fsanitize=address,undefined",
    "-I"+str(source),"-I"+str(source/"plugins/backends"),"-I"+str(source/"plugins/backends/aqhbci"),
    str(ROOT/"native/aqbanking/tests.c"),*flags,"-o",str(binary)],check=True)
with tempfile.TemporaryDirectory(prefix="bankingcraft-adapter-") as directory:
    blocker=Path(directory)/"blocked";blocker.touch()
    env["HOME"]=directory
    # Device-name entry, forced replacement, cancelled replacement, validation,
    # then repair of an older initialized snapshot. All names/configs are fake.
    medium_answers=[dict(op="answer",value=" BestSign Gerät "),
        dict(op="answer",value="7920"),dict(op="answer",value="Second Device"),
        dict(op="answer",value="7920"),dict(op="answer",cancel="1"),
        dict(op="answer",value="7920"),dict(op="answer",value="X"*33),dict(op="answer",value="Ü"*32),
        dict(op="answer",value="Repaired Device")]
    bestsign_answers=[dict(op="answer",value="7921"),dict(op="answer",value="Repaired Device")]
    approval_answers=[*[dict(op="answer",value="") for _ in range(5)],dict(op="answer",cancel="1"),
                      dict(op="answer",value=""),dict(op="answer",value=""),dict(op="answer",value="")]
    for answer,expected in [(dict(op="answer",value="1"),b"1"),(dict(op="answer",value="0"),b"0"),(dict(op="answer",cancel="1"),b"0")]:
        # This process runs the entire instrumented C suite, including plugin
        # initialization. Intel hosted Macs need more than a single IPC timeout.
        result=subprocess.run([str(binary),str(blocker)],input=b"".join(wire.encode(a) for a in [*medium_answers,*bestsign_answers,*approval_answers,answer]),capture_output=True,env=env,timeout=60)
        if result.returncode: raise RuntimeError(result.stderr.decode())
        frames=wire.decode(result.stdout)
        reader=[f for f in frames if f["event"]==b"show" and f.get("kind")==b"reader_pin"]
        assert len(reader)==2 and len({f["id"] for f in reader})==2
        assert all(b"PIN" in f["text"] and b"Kartenleser" in f["text"] for f in reader)
        assert [f["id"] for f in frames if f["event"]==b"hide"]==[f["id"] for f in reader]
        reads=[f for f in frames if f["event"]==b"read_result"]
        assert [(f["account"],f["balance_code"],f["transactions_code"]) for f in reads]==[
            (b"42",b"0",b"0"),(b"43",b"0",b"0"),(b"42",b"0",b"0"),(b"43",b"-1",b"-1"),
            (b"42",b"0",b"0"),(b"44",b"0",b"-68")]
        prompts=[frame for frame in frames if frame["event"]==b"password"]
        approvals=[frame for frame in prompts if frame.get("method")==b"7"]
        assert len(approvals)==9
        assert approvals[-1]["title"]==b"BestSign" and "automatic" not in approvals[-1]
        approvals=approvals[:-1]
        assert [f["attempt"] for f in approvals]==[b"1",b"2",b"3",b"4",b"5",b"6",b"6",b"7"]
        assert [f["wait_seconds"] for f in approvals]==[b"2"]+[b"3"]*7
        assert all(f["automatic"]==b"1" and f["limit"]==b"7" for f in approvals)
        assert approvals[0]["text"]==b"SYNTHETIC BESTSIGN CHALLENGE"
        assert all(frame["title"]==b"BestSign-Push" and frame["min"]==b"0" and frame["max"]==b"1" for frame in approvals)
        prompts=[frame for frame in prompts if frame.get("method")!=b"7"]
        assert len(prompts)==7
        bestsign=next(f for f in frames if f["event"]==b"choice" and b"Postbank" in f.get("text",b""))
        assert bestsign["7921"] and all(key not in bestsign for key in ("6920","7920","7900","7999"))
        assert all(frame["flags"]==b"2" and frame["title"]=="Name des TAN-Mediums".encode() for frame in prompts)
        vop=next(frame for frame in frames if frame["event"]==b"vop")
        assert b"SYNTHETIC FULL BANK NOTICE" in vop["text"]
        assert b"DE02120300000000202051" in vop["text"]
        assert frames[-1]["accepted"]==expected
        # Correct a bad device even when setup has not completed. The real
        # worker must reach this local dialog without attempting bank login.
        request=dict(version="1",op="reset_tan",data_path=str(blocker),state=frames[-1]["state"],
            blz="12345678",user="synthetic-user",customer="synthetic-user",medium="pintan",record="1",
            url="https://bank.invalid",product="SYNTHETICREGISTRATION0001",product_version="1")
        repair=subprocess.run([str(helper)],
            input=wire.encode(request)+wire.encode(dict(op="answer",value="7921"))+
                wire.encode(dict(op="answer",value="Fixed Before Login")),
            stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,env=env,timeout=10)
        assert repair.returncode==0
        repair_frames=wire.decode(repair.stdout)
        assert [f["event"] for f in repair_frames if f["event"] in (b"choice",b"password")]==[b"choice",b"password"]
        assert repair_frames[-1]["code"]==b"0" and b"Fixed Before Login" in repair_frames[-1]["state"]
        # Reproduce the installed user's old 920/v6 choice in an independent
        # worker. A read-only open must migrate it before bank I/O, without
        # another device, PIN or TAN prompt; test only synthetic state.
        ready_state=next(f["state"] for f in frames if f["event"]==b"ready")
        legacy_state,changed=re.subn(rb'(selectedTanMethod\s*=\s*"?)7921(?![0-9])',
                                    lambda m:m[1]+b'6920',ready_state)
        assert changed==1
        migrated=subprocess.run([str(helper)],input=wire.encode(dict(request,op="open",state=legacy_state,preferred_tan="bestsign")),
                                stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,env=env,timeout=10)
        assert migrated.returncode==0
        migrated_frames=wire.decode(migrated.stdout)
        assert migrated_frames[-1]["code"]==b"0"
        assert re.search(rb'selectedTanMethod\s*=\s*"?7921(?![0-9])',migrated_frames[-1]["state"])
        assert b"Repaired Device" in migrated_frames[-1]["state"]
        assert all(f["event"] not in (b"choice",b"password",b"message") for f in migrated_frames)
        # Restoring the ready snapshot and rebuilding capabilities must work
        # in a fresh worker without any socket/PIN/TAN request.
        request.update(op="open",state=next(f["state"] for f in frames if f["event"]==b"ready"))
        opened=subprocess.run([str(helper)],input=wire.encode(request),stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,env=env,timeout=10)
        assert opened.returncode==0
        current=wire.decode(opened.stdout)
        assert current[-1]["code"]==b"0"
        assert all(f["event"] not in (b"choice",b"password",b"message") for f in current)
        card=next(f for f in current if f["event"]==b"account" and f["number"]==b"1234567890")
        assert b"HKKAZ" in card["allowed"]  # native GetTransactions capability, card-specific job
        unsupported=next(f for f in current if f["event"]==b"account" and f["sub"]==b"02")
        assert not unsupported["allowed"]
        request.update(op="transactions",account=unsupported["id"],state=current[-1]["state"])
        rejected=subprocess.run([str(helper)],input=wire.encode(request),stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,env=env,timeout=10)
        assert rejected.returncode==0
        rejected_frames=wire.decode(rejected.stdout)
        done=rejected_frames[-1]
        assert done["code"]==b"-68" and done["unsupported_account_job"]==b"1"
        assert b"keinen Umsatzabruf" in done["notice"]
        assert any(f["event"]==b"account" and f["id"]==unsupported["id"] and not f["allowed"] for f in rejected_frames)
        # A card refresh retains one worker across read-only requests. Use the
        # ready synthetic state and unsupported account to avoid any card/bank IO.
        request.update(op="open",medium="zkacard",card_session="1",state=current[-1]["state"])
        follow=[dict(op="transactions",account=unsupported["id"]),dict(op="open"),dict(op="close")]
        session=subprocess.run([str(helper)],input=wire.encode(request)+b"".join(wire.encode(f) for f in follow),
            stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,env=env,timeout=10)
        assert session.returncode==0
        done=[f for f in wire.decode(session.stdout) if f["event"]==b"done"]
        assert [f["code"] for f in done]==[b"0",b"-68",b"0"]
        assert all(f["session"]==b"1" for f in done)
        # Neither payment dispatch nor a changed access identity may enter the
        # existing card session. Rejection happens before setup or network IO.
        for forbidden in [dict(op="transfer"),dict(op="open",user="another-user"),dict(op="open",state=b"")]:
            result=subprocess.run([str(helper)],input=wire.encode(request)+wire.encode(forbidden),
                stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,env=env,timeout=10)
            assert result.returncode==0
            done=[f for f in wire.decode(result.stdout) if f["event"]==b"done"]
            assert len(done)==2 and done[-1]["code"]!=b"0" and done[-1]["session"]==b"0"
        # EOF also closes/unmounts normally instead of waiting for another job.
        eof=subprocess.run([str(helper)],input=wire.encode(request),stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,env=env,timeout=10)
        assert eof.returncode==0
print("PASS: exact HKVVB product field for five protocol/segment variants, separate application name, card context matching, card-session reuse/teardown/read-only boundaries, structured reader PIN begin/end notifications, multi-account read batching/account-specific dates and results/unsupported jobs/no retries, CAMT entry amounts/currencies/signs/optional details, unused empty balances, malformed amount rejection, socket-wait progress suppression, repeated snapshot groups/upgrade, cross-process card capabilities, statement-factory fallback, unsupported-job details, TAN-medium loading/entry/persistence/cancel/repair, BestSign legacy-vs-push classification/native-dialog selection/cross-process migration/device preservation/BPD refresh/approval/pending/success/rejection/cancel/HKVPA-once/status-only polling/bank intervals/poll limit/manual fallback, CAMT/MT940/card jobs, SEPA import/identity/migration/retry, SCA, TLS and VoP, Postbank match-only dialog omission and mismatch/missing/partial-result prompts; ASan/UBSan adapter")
