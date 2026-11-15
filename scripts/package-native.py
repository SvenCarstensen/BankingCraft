#!/usr/bin/env python3
"""Bundle the worker, data, plugins, dependency inventory and shared libraries.

The destination is the application executable directory. No signing identity,
notarization or update mechanism is added here; macOS ad-hoc signatures only
allow the relocated local Mach-O images to load.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess

ROOT=Path(__file__).resolve().parents[1]

def output(*args):
    return subprocess.check_output(args,text=True).strip()

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination",type=Path)
    args=parser.parse_args()
    prefix=Path((ROOT/".native/install-prefix").read_text().strip())
    destination=args.destination.resolve()/"native"
    if destination.exists():
        raise SystemExit("Native destination already exists; package into a new staging directory")
    (destination/"bin").mkdir(parents=True)
    (destination/"lib").mkdir()
    windows=platform.system()=="Windows"
    executable="bankingcraft-aqbanking"+(".exe" if windows else "")
    shutil.copy2(prefix/"bin"/executable,destination/"bin"/executable)
    for directory in ["share","etc"]:
        if (prefix/directory).exists():
            shutil.copytree(prefix/directory,destination/directory)
    for path in (prefix/"lib").rglob("*"):
        if path.is_file() and (".so" in path.name or path.suffix in [".dylib",".dll",".xml"]):
            target=destination/"lib"/path.relative_to(prefix/"lib")
            target.parent.mkdir(parents=True,exist_ok=True)
            shutil.copy2(path,target)
    binaries=[p for p in destination.rglob("*") if p.is_file() and
              (p==destination/"bin"/executable or p.suffix in [".dylib",".dll"] or ".so" in p.name)]
    queue=list(binaries); processed=set(); inventory={}
    while queue:
        binary=queue.pop()
        if binary in processed: continue
        processed.add(binary)
        links=[]
        if platform.system()=="Darwin":
            links=[line.strip().split(" (",1)[0] for line in output("otool","-L",str(binary)).splitlines()[1:]]
            identities=output("otool","-D",str(binary)).splitlines()[1:]
            links=[p for p in links if p not in identities]
            links=[p for p in links if not p.startswith(("/System/Library/","/usr/lib/"))]
        elif windows:
            names=re.findall(r"DLL Name:\s*(\S+)",output("objdump","-p",str(binary)))
            for name in names:
                candidates=[prefix/"bin"/name,Path(os.environ.get("MINGW_PREFIX","/mingw64"))/"bin"/name]
                source=next((p for p in candidates if p.is_file()),None)
                if source: links.append(str(source))
                elif not (Path(os.environ["WINDIR"])/"System32"/name).is_file():
                    if not name.lower().startswith(("api-ms-","ext-ms-")): raise SystemExit("Unresolved DLL: "+name)
        else:
            listing=output("ldd",str(binary))
            if "not found" in listing: raise SystemExit("Unresolved native library: "+str(binary))
            links=re.findall(r"=>\s+(/\S+)",listing)
            # glibc/loader are supplied by the host, matching AppImage policy.
            links=[p for p in links if not re.match(r"lib(c|m|pthread|dl|rt|resolv)\.so",Path(p).name)]
        for linked in links:
            if linked.startswith("@"):
                raise SystemExit("Unexpected unresolved Mach-O path: "+linked)
            source=Path(linked).resolve()
            target=destination/("bin" if windows else "lib")/source.name
            if not target.exists():
                shutil.copy2(source,target);queue.append(target)
            elif hashlib.sha256(target.read_bytes()).digest()!=hashlib.sha256(source.read_bytes()).digest() and target not in processed:
                raise SystemExit("Conflicting library name: "+source.name)
            inventory[str(target.relative_to(destination))]={"source":str(source),"sha256":hashlib.sha256(source.read_bytes()).hexdigest()}
            if platform.system()=="Darwin":
                relative=os.path.relpath(target,binary.parent)
                subprocess.run(["install_name_tool","-change",linked,"@loader_path/"+relative,str(binary)],check=True)
        if platform.system()=="Darwin":
            subprocess.run(["codesign","--force","--sign","-",str(binary)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        elif not windows:
            relative=os.path.relpath(destination/"lib",binary.parent)
            subprocess.run(["patchelf","--set-rpath","$ORIGIN/"+relative,str(binary)],check=True)
    if platform.system()=="Darwin":
        minimums=["11.0"]
        for binary in processed:
            for block in output("otool","-l",str(binary)).split("Load command"):
                field="minos" if "cmd LC_BUILD_VERSION" in block else "version" if "cmd LC_VERSION_MIN_MACOSX" in block else None
                if field: minimums.extend(re.findall(r"\b"+field+r"\s+(\d+\.\d+(?:\.\d+)?)",block))
        (destination/"minimum-macos.txt").write_text(max(minimums,key=lambda v:tuple(map(int,v.split("."))))+"\n")
    licenses=destination/"licenses";licenses.mkdir()
    for package in json.loads((ROOT/"native/dependencies.json").read_text()):
        source=prefix.parent/(package["name"]+"-"+package["version"])
        folder=licenses/package["name"];folder.mkdir()
        for name in ["COPYING","COPYING.LESSER","AUTHORS","README"]:
            if (source/name).is_file(): shutil.copy2(source/name,folder/name)
    if platform.system()=="Darwin":
        # Preserve package identities for vulnerability scanning, and original
        # notices for the Homebrew libraries actually entering this package.
        formulas=json.loads(output("brew","info","--json=v2","--installed"))["formulae"]
        used=[]
        for formula in formulas:
            if not any("/Cellar/"+formula["name"]+"/" in item["source"] for item in inventory.values()): continue
            used.append(formula)
            location=Path(output("brew","--prefix",formula["name"]))
            folder=licenses/formula["name"];folder.mkdir(exist_ok=True)
            for path in location.rglob("*"):
                if path.is_file() and re.match(r"^(LICENSE|COPYING|COPYRIGHT|NOTICE)([-_.].*)?$",path.name,re.I):
                    target=folder/path.relative_to(location);target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(path,target)
        (destination/"homebrew.json").write_text(json.dumps(used,indent=2)+"\n")
    (destination/"dependencies.json").write_bytes((ROOT/"native/dependencies.json").read_bytes())
    (destination/"runtime-files.json").write_text(json.dumps(inventory,indent=2)+"\n")
    if platform.system()=="Linux":
        packages={}
        os_release=dict(line.split("=",1) for line in Path("/etc/os-release").read_text().splitlines() if "=" in line)
        distro=os_release["ID"].strip('"')+"-"+os_release["VERSION_ID"].strip('"')
        for item in inventory.values():
            source=item["source"]
            if source.startswith(str(prefix)): continue
            owners=subprocess.run(["dpkg-query","-S",source],capture_output=True,text=True)
            if owners.returncode and source.startswith("/usr/lib/"):
                owners=subprocess.run(["dpkg-query","-S",source.removeprefix("/usr")],capture_output=True,text=True)
            if owners.returncode: raise SystemExit("Cannot identify packaged library owner: "+source)
            owner=owners.stdout.split(": ",1)[0]
            name,version=output("dpkg-query","-W","-f=${Package}\t${Version}",owner).split("\t")
            packages[name]={"name":name,"version":version,"distro":distro}
            notice=Path("/usr/share/doc")/name/"copyright"
            if notice.is_file(): shutil.copy2(notice,licenses/(name+"-copyright.txt"))
        (destination/"system-packages.json").write_text(json.dumps(list(packages.values()),indent=2)+"\n")
    if windows:
        mingw=Path(os.environ.get("MINGW_PREFIX","C:/msys64/mingw64"))
        pacman=mingw.parent/"usr/bin/pacman.exe"
        cygpath=mingw.parent/"usr/bin/cygpath.exe"
        packages={}
        for item in inventory.values():
            source=Path(item["source"])
            if source.is_relative_to(prefix): continue
            unix=output(str(cygpath),"-u",str(source))
            name=output(str(pacman),"-Qqo",unix)
            if name in packages: continue
            _,version=output(str(pacman),"-Q",name).split()
            packages[name]={"name":name,"version":version,"upstream_name":name.removeprefix("mingw-w64-x86_64-"),
                            "upstream_version":version.rsplit("-",1)[0]}
            for filename in output(str(pacman),"-Qlq",name).splitlines():
                if "/share/licenses/" not in filename or filename.endswith("/"): continue
                original=Path(output(str(cygpath),"-m",filename))
                if original.is_file():
                    target=licenses/"msys2"/filename.split("/share/licenses/",1)[1]
                    target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(original,target)
        (destination/"msys2-packages.json").write_text(json.dumps(list(packages.values()),indent=2)+"\n")
    subprocess.run([os.sys.executable,str(ROOT/"scripts/test-native.py"),str(destination/"bin"/executable)],check=True)
    print(destination)

if __name__=="__main__": main()
