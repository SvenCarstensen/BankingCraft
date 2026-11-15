#!/usr/bin/env python3
"""Inventory and scan the packaged native runtime, including transitive libraries.

Only public vulnerability databases/tool binaries are downloaded. The software
inventory is evaluated locally; no vault or bank data is read or uploaded.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import urllib.request
import zipfile
from urllib.parse import quote

ROOT=Path(__file__).resolve().parents[1]

def tool(name):
    spec=json.loads((ROOT/"native/audit-tools.json").read_text())[name]
    machine={"arm64":"arm64","aarch64":"arm64","x86_64":"amd64","AMD64":"amd64"}[platform.machine()]
    asset=spec["assets"][platform.system().lower()+"_"+machine]
    folder=Path.home()/".cache/bankingcraft/audit"/(name+"-"+spec["version"])
    folder.mkdir(parents=True,exist_ok=True)
    archive=folder/asset["url"].rsplit("/",1)[1]
    if not archive.exists():
        with urllib.request.urlopen(asset["url"],timeout=60) as remote:
            content=remote.read(150*1024*1024+1)
        if len(content)>150*1024*1024: raise RuntimeError("Audit tool exceeds size limit")
        archive.write_bytes(content)
    if hashlib.sha256(archive.read_bytes()).hexdigest()!=asset["sha256"]:
        raise RuntimeError("Audit tool checksum mismatch")
    executable=name+(".exe" if os.name=="nt" else "")
    if archive.suffix==".zip":
        with zipfile.ZipFile(archive) as bundle: binary=bundle.read(executable)
    else:
        with tarfile.open(archive) as bundle: binary=bundle.extractfile(executable).read()
    path=folder/executable;path.write_bytes(binary);path.chmod(0o700)
    return str(path)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runtime",type=Path)
    parser.add_argument("--output",type=Path,default=ROOT/".native/audit")
    args=parser.parse_args();runtime=args.runtime.resolve();args.output.mkdir(parents=True,exist_ok=True)
    if not (runtime/"runtime-files.json").is_file():
        raise SystemExit("Scan a runtime produced by scripts/package-native.py, including its dependencies")
    syft,grype=tool("syft"),tool("grype")
    bom=args.output/"native.cdx.json"
    subprocess.run([syft,"scan","dir:"+str(runtime),"-o","cyclonedx-json="+str(bom)],check=True)
    data=json.loads(bom.read_text())
    # Add pinned source identities even if binary signature catalogers miss them.
    packages=json.loads((runtime/"dependencies.json").read_text())
    brew=runtime/"homebrew.json"
    if brew.exists():
        packages += [{"name":p["name"],"version":p["installed"][0]["version"].split("_")[0],
                      "url":p["urls"]["stable"]["url"]} for p in json.loads(brew.read_text())]
    msys=runtime/"msys2-packages.json"
    if msys.exists():
        packages += [{"name":p["upstream_name"],"version":p["upstream_version"],
                      "url":"https://packages.msys2.org/package/"+p["name"]} for p in json.loads(msys.read_text())]
    for package in packages:
        data.setdefault("components",[]).append({"type":"library","name":package["name"],"version":package["version"],
            "bom-ref":"bankingcraft-native:"+package["name"]+":"+package["version"],
            "purl":"pkg:generic/"+package["name"]+"@"+package["version"],
            "externalReferences":[{"type":"distribution","url":package["url"]}]})
    system=runtime/"system-packages.json"
    if system.exists():
        for package in json.loads(system.read_text()):
            purl="pkg:deb/"+package["distro"].split("-",1)[0]+"/"+package["name"]+"@"+quote(package["version"],safe="")+"?distro="+package["distro"]
            data["components"].append({"type":"library","name":package["name"],"version":package["version"],"purl":purl,"bom-ref":purl})
    bom.write_text(json.dumps(data,indent=2)+"\n")
    with (args.output/"native-vulnerabilities.json").open("w") as report:
        result=subprocess.run([grype,"sbom:"+str(bom),"--add-cpes-if-none","--fail-on","low","-o","json"],stdout=report)
    (args.output/"tools.json").write_bytes((ROOT/"native/audit-tools.json").read_bytes())
    if result.returncode: raise SystemExit(result.returncode)
    print("Native dependency scan passed; see",args.output)

if __name__=="__main__": main()
