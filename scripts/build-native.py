#!/usr/bin/env python3
"""Build pinned AqBanking/Libchipcard sources and BankingCraft's isolated worker.

Prerequisites: C/C++ compiler, make, pkg-config, GnuTLS, libgcrypt, GMP,
libxml2/xmlsec, gettext, and PC/SC development headers (macOS: system SDK).
Windows: run in an MSYS2 UCRT64 environment; the Rust executable may use MSVC.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]

def replace_once(path, old, new):
    content = path.read_text()
    if new in content:
        return
    if content.count(old) != 1:
        raise RuntimeError(f"Upstream source changed: {path.name}; review the patch")
    path.write_text(content.replace(old, new))

def patch_source(name, source):
    if name == "aqbanking":
        # Linux LeakSanitizer exposes three ownership bugs in pinned 6.9.2.
        # Free temporary objects at their owners; keep leak detection enabled.
        replace_once(source / "src/libs/plugins/backends/aqhbci/joblayer/jobqueue_account.c",
                     '    accountSpecList=AB_AccountSpec_List_new();\n    rv=AB_Banking_GetAccountSpecList(ab, &accountSpecList);',
                     '    /* GetAccountSpecList allocates the returned list. */\n    rv=AB_Banking_GetAccountSpecList(ab, &accountSpecList);')
        replace_once(source / "src/libs/aqbanking/banking_imex.c",
                     '  } /* while(seGlobalDataDir) */\n  GWEN_Buffer_free(pathBuffer);',
                     '  } /* while(seGlobalDataDir) */\n  GWEN_StringList_free(slGlobalDataDirs);\n  GWEN_Buffer_free(pathBuffer);')
        replace_once(source / "src/libs/plugins/backends/aqhbci/joblayer/job_swift.c",
                     '      DBG_INFO(AQHBCI_LOGDOMAIN, "Param [%s] does not match family %s.%d", sName, family, version1);\n    }',
                     '      DBG_INFO(AQHBCI_LOGDOMAIN, "Param [%s] does not match family %s.%d", sName, family, version1);\n    }\n    AB_SwiftDescr_free(tmpDescr);')
        # camt.052.001.08 creates one transaction per Ntry, but 6.9.2 reads
        # its amount inside optional NtryDtls/TxDtls. Banks can omit those
        # details, and a detail/foreign-currency amount is not the booked
        # entry amount. Keep this correction inside AqBanking's XML profile.
        camt = source / "src/libs/plugins/imexporters/xml/data/camt_052_001_08.xml"
        for sign, direction in [("-", "DBIT"), ("", "CRDT")]:
            replace_once(camt,
                         '''                        <XmlIfHasCharData path="Amt">
                          <XmlIfHasCharData path="Amt@Ccy">
                            <DbCreateAndEnterTempGroup name="amount" >
                              <DbSetTempCharValue name="value"    path="Amt" />
                              <DbSetTempCharValue name="currency" path="Amt@Ccy" />
                              <DbSetCharValue name="value" value="''' + sign + '''$(value):$(currency)" />
                            </DbCreateAndEnterTempGroup> <!-- amount -->
                          </XmlIfHasCharData> <!-- Amt@Ccy -->
                        </XmlIfHasCharData>   <!-- Amt -->''',
                         "                        <!-- " + direction + " amount is read from Ntry above. -->")
        amounts = "                  <!-- BankingCraft: booked entry amount, independent of optional details. -->\n"
        for sign, direction in [("-", "DBIT"), ("", "CRDT")]:
            amounts += '''                  <XmlIfCharDataMatches path="CdtDbtInd" pattern="''' + direction + '''">
                    <XmlIfHasCharData path="Amt@Ccy">
                      <DbCreateAndEnterTempGroup name="amount">
                        <DbSetTempCharValue name="value" path="Amt" />
                        <DbSetTempCharValue name="currency" path="Amt@Ccy" />
                        <DbSetCharValue name="value" value="''' + sign + '''$(value):$(currency)" mode="replace" />
                      </DbCreateAndEnterTempGroup>
                    </XmlIfHasCharData>
                  </XmlIfCharDataMatches>
'''
        replace_once(camt, "                  <!-- booking date -->", amounts + "\n                  <!-- booking date -->")
        # A legacy HKKAZ permission may survive in UPD after HIKAZS disappears
        # from BPD. The first advertised code must not hide a supported card
        # job later in the list. These are local factory attempts, not retries
        # of requests sent to the bank.
        replace_once(source / "src/libs/plugins/backends/aqhbci/ajobs/jobgettransactions.c",
                     '''  int jobType;

  jobType=_determineJobToUse(u, account);
  switch(jobType) {
  case AH_JOB_GETTRANS_HKKAZ: return _createJob_HKKAZ(pro, u, account);
  case AH_JOB_GETTRANS_DKKKU: return _createJob_DKKKU(pro, u, account);
  case AH_JOB_GETTRANS_HKKKU: return _createJob_HKKKU(pro, u, account);
  case AH_JOB_GETTRANS_HKWDU: return _createJob_HKWDU(pro, u, account);
  default:
    DBG_NOTICE(AQHBCI_LOGDOMAIN, "No FinTS job (GetTransaction) available for this account");
    return NULL;
  }''',
                     '''  GWEN_DB_NODE *updgroup=AH_User_GetUpdForAccount(u, account);
  GWEN_DB_NODE *n=updgroup?GWEN_DB_GetFirstGroup(updgroup):NULL;
  for (; n; n=GWEN_DB_GetNextGroup(n)) {
    const char *code=GWEN_DB_GetCharValue(n, "job", 0, "");
    AH_JOB *job=NULL;
    if (strcasecmp(code, "HKKAZ")==0)
      job=_createJob_HKKAZ(pro, u, account);
    else if (strcasecmp(code, "DKKKU")==0)
      job=_createJob_DKKKU(pro, u, account);
    else if (strcasecmp(code, "HKKKU")==0)
      job=_createJob_HKKKU(pro, u, account);
    else if (strcasecmp(code, "HKWDU")==0)
      job=_createJob_HKWDU(pro, u, account);
    if (job)
      return job;
  }
  DBG_NOTICE(AQHBCI_LOGDOMAIN, "No FinTS job (GetTransaction) available for this account");
  return NULL;''')
        # Upstream master already guards this automatic legacy import with
        # !dname. 6.9.2 must not read or copy another application's bank config.
        path = source / "src/libs/aqbanking/banking.c"
        replace_once(path, "  {\n    int rv;\n\n    rv=AB_Banking_CopyOldSettingsFolderIfNeeded(ab);",
                     "  if (!dname) {\n    int rv;\n\n    rv=AB_Banking_CopyOldSettingsFolderIfNeeded(ab);")
        content = path.read_text()
        if "AB_BankingCraft_NoLegacyImport" not in content:
            path.write_text(content + "\nint AB_BankingCraft_NoLegacyImport(void) { return 1; }\n")
        # Never create raw FinTS logs. Besides containing credentials, the
        # upstream failure branch frees its path buffer twice. Keep the regular
        # file blocker as defence for any other attempted filesystem writes.
        path = source / "src/libs/plugins/backends/aqhbci/msglayer/dialog.c"
        content = path.read_text()
        marker = "  /* BankingCraft: raw banking logs disabled. */"
        if marker not in content:
            start = content.index("  /* create path */")
            end = content.index("\n  return dlg;", start)
            path.write_text(content[:start] + marker + "\n" + content[end:])
        # Always show the bank's VoP result, including matches. No implicit
        # opt-out when the bank returns no report or no explanatory message.
        path = source / "src/libs/plugins/backends/aqhbci/applayer/cbox_vopmsg.c"
        replace_once(path,
                     '    DBG_WARN(AQHBCI_LOGDOMAIN, "No msg, no non-matching results, silently accepting.");\n    return 0;',
                     '    if (resultList==NULL) sMsg="Keine Empfängerprüfung verfügbar. Auftrag trotzdem freigeben?";')
        replace_once(source / "src/libs/plugins/backends/aqhbci/tan/tanmechanism.c",
                     'tanMechanism= AH_TanMechanism_ChipTanUSB_new(tanMethod, id);',
                     'tanMechanism= AH_TanMechanism_ChipTanOpt_new(tanMethod, AB_BANKING_TANMETHOD_CHIPTAN_OPTIC);')
        # The 6.9.2 loader checks only the legacy HITANS v3 J/N field. v4+
        # uses needTanMediumId with 0=forbidden, 1=optional, 2=required.
        # Keep parsing in AqBanking and expose its existing boolean API correctly.
        replace_once(source / "src/libs/plugins/backends/aqhbci/banking/user.c",
                     '            s=GWEN_DB_GetCharValue(dbT, "tanMediumIdNeeded", 0, NULL);\n'
                     '            if (s && strcasecmp(s, "j")==0)\n'
                     '              AH_TanMethod_SetNeedTanMediumId(tm, 1);',
                     '            s=GWEN_DB_GetCharValue(dbT, "needTanMediumId", 0, NULL);\n'
                     '            if (s) {\n'
                     '              AH_TanMethod_SetNeedTanMediumId(tm, strcmp(s, "2")==0);\n'
                     '            }\n'
                     '            else {\n'
                     '              s=GWEN_DB_GetCharValue(dbT, "tanMediumIdNeeded", 0, NULL);\n'
                     '              if (s && strcasecmp(s, "j")==0)\n'
                     '                AH_TanMethod_SetNeedTanMediumId(tm, 1);\n'
                     '            }')
    if name == "libchipcard":
        # pkg-config 0.29 (Ubuntu 22.04 releases) expands variables in order.
        # Both upstream templates reference datadir before defining it.
        for kind, variable in [("client", "lc_client_xml_dir"), ("server", "lc_server_data_dir")]:
            replace_once(source / f"libchipcard-{kind}.pc.in",
                         f"pkgdatadir=@{variable}@\ndatarootdir=@datarootdir@\ndatadir=@datadir@",
                         f"datarootdir=@datarootdir@\ndatadir=@datadir@\npkgdatadir=@{variable}@")
        # Current Apple C++ compilers reject concatenation without whitespace.
        replace_once(source / "src/tools/cardcommander/cardcommander.cpp",
                     'v"k_CHIPCARD_VERSION_STRING"', 'v" k_CHIPCARD_VERSION_STRING "')
        # Preserve the user's explicit reader selection inside Libchipcard's
        # discovery loop. No card commands are implemented by BankingCraft.
        path = source / "src/lib/client/base/client.c"
        replace_once(path, "          /* card inserted and not used by another application */",
                     '          const char *selected=getenv("BANKINGCRAFT_CARD_READER");\n'
                     '          if (selected && *selected && strcmp(selected,cl->readerStates[i].szReader)) continue;\n'
                     "          /* card inserted and not used by another application */")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path, default=Path.home() / ".cache/bankingcraft/native")
    parser.add_argument("--jobs", type=int, default=min(os.cpu_count() or 2, 8))
    args = parser.parse_args()
    work = args.work.resolve()
    if any(c.isspace() for c in str(work)):
        raise RuntimeError("Native autotools builds require a work directory without whitespace")
    prefix = work / "install"
    prefix.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["PKG_CONFIG_PATH"] = str(prefix / "lib/pkgconfig") + os.pathsep + env.get("PKG_CONFIG_PATH", "")
    env["PATH"] = str(prefix / "bin") + os.pathsep + env["PATH"]
    env["LDFLAGS"] = "-L" + (prefix / "lib").as_posix() + " " + env.get("LDFLAGS", "")
    shell = None
    make_tool = "make"
    if os.name == "nt":
        # CreateProcess searches System32 before PATH: bare "bash" starts WSL
        # on hosted runners. Use the MSYS2 installation owning this Python.
        toolchain = Path(sys.executable).resolve().parents[1]
        msys_bin = toolchain.parent / "usr/bin"
        shell = msys_bin / "bash.exe"
        if not shell.is_file():
            raise RuntimeError("Run build-native.py with MSYS2 UCRT64 Python")
        make_tool = str(msys_bin / "make.exe")
        env["SHELL"] = shell.as_posix()
        env["PATH"] = os.pathsep.join([str(prefix / "bin"), str(toolchain / "bin"), str(msys_bin), env["PATH"]])
    if sys.platform == "darwin":
        sdk = subprocess.check_output(["xcrun", "--show-sdk-path"], text=True).strip()
        flags = " -I" + shlex.quote(sdk + "/System/Library/Frameworks/PCSC.framework/Headers")
        env["CFLAGS"] = env.get("CFLAGS", "-O2") + flags
        # Homebrew's keg-only libraries must be discoverable by configure.
        brew = subprocess.check_output(["brew", "--prefix"], text=True).strip()
        env["PATH"] = brew + "/opt/gettext/bin:" + env["PATH"]
        env["CPPFLAGS"] = env.get("CPPFLAGS", "") + " -I" + brew + "/include -I" + brew + "/opt/gettext/include"
        env["LDFLAGS"] = env.get("LDFLAGS", "") + " -L" + brew + "/lib -L" + brew + "/opt/gettext/lib"
        env["PKG_CONFIG_PATH"] += ":" + ":".join(brew + "/opt/" + p + "/lib/pkgconfig" for p in ["openssl@4", "libxml2", "gettext"])
    packages = json.loads((ROOT / "native/dependencies.json").read_text())
    for package in packages:
        stem = package["name"] + "-" + package["version"]
        vendored = ROOT / "native/vendor" / (stem + ".tar.gz")
        archive = vendored if vendored.is_file() else work / (stem + ".tar.gz")
        if not archive.exists():
            print("Downloading", stem, flush=True)
            with urllib.request.urlopen(package["url"], timeout=60) as remote:
                data = remote.read(40 * 1024 * 1024 + 1)
            if len(data) > 40 * 1024 * 1024:
                raise RuntimeError("Source archive exceeds size limit")
            archive.write_bytes(data)
        if hashlib.sha256(archive.read_bytes()).hexdigest() != package["sha256"]:
            raise RuntimeError(f"Source checksum mismatch: {stem}")
        source = work / stem
        if not source.exists():
            with tarfile.open(archive) as tar:
                tar.extractall(work, filter="data")
        patch_source(package["name"], source)
        stamp = source / ".bankingcraft-built-v2"
        recipe = hashlib.sha256(Path(__file__).read_bytes() + (ROOT / "native/dependencies.json").read_bytes()).hexdigest()
        if not stamp.exists() or stamp.read_text().strip() != recipe:
            print("Building", stem, flush=True)
            package_env = env.copy()
            make = [make_tool]
            configure = ([str(shell)] if shell else []) + ["./configure", "--prefix=" + prefix.as_posix(), "--disable-static", "--disable-full-doc", "--enable-local-install"]
            if package["name"] == "gwenhywfar":
                # binreloc reads /proc symlinks and is unavailable in native
                # Windows; Gwenhywfar has a separate Win32 path implementation.
                configure += ["--with-guis=none", "--disable-binreloc" if os.name == "nt" else "--enable-binreloc"]
            if package["name"] == "aqbanking":
                configure += ["--with-backends=aqhbci aqnone"]
                # The isolated worker uses the pinned AqHBCI/backend API, which
                # upstream hides on ELF. Export it from our bundled library,
                # as the macOS build already does; never use a system AqBanking.
                package_env["ac_cv_hidden_visibility_attribute"] = "no"
                if os.name == "nt":
                    # Explicit public dllexports disable MinGW's automatic exports.
                    # The worker also needs the internal AB_/AH_ entry points.
                    package_env["LDFLAGS"] += " -Wl,--export-all-symbols"
            with (work / (stem + ".build.log")).open("w") as log:
                for command in [configure, make + ["clean"], make + ["-j" + str(args.jobs)], make + ["install"]]:
                    try:
                        subprocess.run(command, cwd=source, env=package_env, stdout=log, stderr=subprocess.STDOUT, check=True)
                    except subprocess.CalledProcessError:
                        log.flush()
                        print((work / (stem + ".build.log")).read_text(errors="replace")[-16000:], file=sys.stderr)
                        raise
            stamp.write_text(recipe + "\n")
    print("Building BankingCraft worker", flush=True)
    pkg = ["pkg-config"]
    cflags = shlex.split(subprocess.check_output(pkg + ["--cflags", "aqbanking", "gwenhywfar", "libchipcard-client"], env=env, text=True))
    libs = shlex.split(subprocess.check_output(pkg + ["--libs", "aqbanking", "gwenhywfar", "libchipcard-client"], env=env, text=True))
    if sys.platform.startswith("linux"):
        # pkg-config supplies -L for linking, not a runtime search path.
        libs += ["-Wl,-rpath,$ORIGIN/../lib"]
    executable = prefix / "bin" / ("bankingcraft-aqbanking.exe" if os.name == "nt" or "MINGW" in env.get("MSYSTEM", "") else "bankingcraft-aqbanking")
    subprocess.run([env.get("CC", "cc"), "-std=c11", "-O2", "-Wall", "-Wextra", "-Wno-unused-function", "-Wno-unused-parameter"]
                   + cflags + ["-I" + str(work / "aqbanking-6.9.2/src/libs"), "-I" + str(work / "aqbanking-6.9.2/src/libs/plugins/backends"),
                               "-I" + str(work / "aqbanking-6.9.2/src/libs/plugins/backends/aqhbci"), str(ROOT / "native/aqbanking/main.c")] + libs + ["-o", str(executable)], env=env, check=True)
    (ROOT / ".native").mkdir(exist_ok=True)
    (ROOT / ".native/helper-path").write_text(str(executable) + "\n")
    (ROOT / ".native/install-prefix").write_text(str(prefix) + "\n")
    print(executable)

if __name__ == "__main__":
    main()
