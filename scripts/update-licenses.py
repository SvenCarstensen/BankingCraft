#!/usr/bin/env python3
"""Regenerate the license bundle, or verify it offline with --check."""
import argparse
import hashlib
import html
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
BUNDLE = ROOT / "licenses"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def font_notices(path):
    """Read the original copyright/license strings from a bundled SFNT font."""
    data = path.read_bytes()
    count = struct.unpack_from(">H", data, 4)[0]
    for n in range(count):
        tag, _, offset, _ = struct.unpack_from(">4sIII", data, 12 + n * 16)
        if tag != b"name":
            continue
        _, count, strings = struct.unpack_from(">HHH", data, offset)
        notices = set()
        for i in range(count):
            platform, _, _, name, size, start = struct.unpack_from(
                ">HHHHHH", data, offset + 6 + i * 12
            )
            if name not in (0, 13, 14):
                continue
            raw = data[offset + strings + start:offset + strings + start + size]
            notices.add(raw.decode("utf-16-be" if platform in (0, 3) else "mac_roman"))
        return "\n\n".join(sorted(notices))
    raise ValueError(f"No name table in {path.name}")


def check():
    manifest = json.loads((BUNDLE / "manifest.json").read_text())
    for relative, expected in manifest["sha256"].items():
        path = ROOT / relative
        if not path.is_file() or digest(path) != expected:
            raise SystemExit(f"License bundle is stale or incomplete: {relative}. "
                             "Run scripts/update-licenses.py and review the result.")
    print(f"License bundle verified: {manifest['dependency_count']} external packages.")


def generate(cargo_about):
    version = subprocess.check_output([cargo_about, "--version"], text=True).strip()
    if version != "cargo-about 0.9.3":
        raise SystemExit(f"Expected cargo-about 0.9.3, got {version}")
    with tempfile.TemporaryDirectory(prefix="bankingcraft-licenses-") as temp:
        output = Path(temp) / "licenses.json"
        subprocess.run([cargo_about, "generate", "--locked", "--workspace", "--fail",
                        "--format", "json", "-o", str(output)], cwd=ROOT, check=True)
        report = json.loads(output.read_text())
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1"], cwd=ROOT
    ))
    packages = {p["name"]: p for p in metadata["packages"]}

    # Preserve original font/native notices, including obligations not represented
    # by the Rust wrapper's SPDX expression (Hack/Bitstream Vera, FreeType, etc.).
    original_files = {
        "epaint_default_fonts": [
            (f"fonts/{name}", f"fonts/{name}") for name in
            ("OFL.txt", "UFL.txt", "Hack-Regular.txt", "emoji-icon-font-mit-license.txt")
        ],
        "freetype-sys": [
            ("freetype2/LICENSE.TXT", "native/freetype/LICENSE.TXT"),
            ("freetype2/docs/FTL.TXT", "native/freetype/FTL.TXT"),
            ("freetype2/src/bdf/README", "native/freetype/BDF.txt"),
            ("freetype2/src/pcf/README", "native/freetype/PCF.txt"),
        ],
        "ring": [(name, f"native/ring/{name}") for name in
                 ("LICENSE", "LICENSE-other-bits", "LICENSE-BoringSSL", "third_party/fiat/LICENSE")],
    }
    for name, files in original_files.items():
        source = Path(packages[name]["manifest_path"]).parent
        for original, relative in files:
            destination = BUNDLE / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / original, destination)
    font_dir = Path(packages["epaint_default_fonts"]["manifest_path"]).parent / "fonts"
    (BUNDLE / "fonts/FONT-NOTICES.txt").write_text("\n\n".join(
        f"{font.name}\n{'=' * len(font.name)}\n{font_notices(font)}"
        for font in sorted(font_dir.glob("*.ttf"))
    ) + "\n")

    plasma = Path(packages["wayland-protocols-plasma"]["manifest_path"]).parent
    declarations = []
    for path in sorted(plasma.rglob("*.xml")):
        for block in re.findall(r"<!--.*?-->|<copyright>.*?</copyright>",
                                path.read_text(), flags=re.S):
            if "copyright" in block.lower():
                declarations.append(f"{path.relative_to(plasma)}\n{block}")
    if not declarations:
        raise ValueError("Missing Plasma protocol copyright declarations")
    (BUNDLE / "native/plasma-protocols.txt").write_text("\n\n".join(declarations) + "\n")

    # Cargo's graph does not include the statically linked Rust standard library.
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    runtime = BUNDLE / "rust-runtime"
    runtime.mkdir(exist_ok=True)
    for name in ("LICENSE-MIT", "LICENSE-APACHE", "COPYRIGHT-library.html"):
        candidates = [sysroot / name, sysroot / "share/doc/rustc" / name,
                      sysroot / "share/doc/rust" / name]
        source = next((p for p in candidates if p.is_file()), None)
        if source is None:
            raise ValueError(f"Install the Rust documentation component to obtain {name}")
        shutil.copyfile(source, runtime / name)
    rust_version = subprocess.check_output(["rustc", "--version", "--verbose"], text=True)
    (runtime / "toolchain.txt").write_text(rust_version)
    rust_source = sysroot / "lib/rustlib/src/rust"
    library = rust_source / "library"
    if not (library / "vendor").is_dir():
        raise ValueError("Install rust-src to collect the standard library's vendored notices")
    runtime_notices = []
    for path in sorted(library.rglob("*")):
        if (path.is_file() and path.suffix.lower() in ("", ".txt", ".md", ".rst", ".html")
                and re.match(r"^(license|licence|copying|copyright|notice|authors)([-_.].*)?$",
                             path.name, flags=re.I)):
            runtime_notices.append(f"{path.relative_to(rust_source)}\n\n{path.read_text()}")
    if not runtime_notices:
        raise ValueError("No vendored Rust runtime notices found")
    # The runtime's LLVM unwinder may be linked on some desktop targets.
    unwind = rust_source / "src/llvm-project/libunwind/LICENSE.TXT"
    if not unwind.is_file():
        raise ValueError("Rust source distribution is missing libunwind/LICENSE.TXT")
    runtime_notices.append(f"src/llvm-project/libunwind/LICENSE.TXT\n\n{unwind.read_text()}")
    (runtime / "SOURCE-NOTICES.txt").write_text("\n\n".join(runtime_notices))

    supplements = json.loads((BUNDLE / "upstream/sources.json").read_text())
    covered = set()
    for entry in supplements:
        if digest(ROOT / entry["file"]) != entry["sha256"]:
            raise ValueError(f"Changed upstream license: {entry['file']}")
        covered.update(entry["packages"])
    # Do not silently ship cargo-about's generic MIT template without the real
    # copyright notices when a crate omits its license file from its package.
    for license in report["licenses"]:
        if license["id"] == "MIT" and license["source_path"] is None:
            for used in license["used_by"]:
                package = used["crate"]
                key = f"{package['name']}-{package['version']}"
                if key not in covered:
                    raise ValueError(f"Add the upstream MIT copyright notice for {key}")

    def escape(text):
        return html.escape(text.replace("\r\n", "\n").replace("\r", "\n"))
    parts = ['<!doctype html><html lang="en"><meta charset="utf-8">',
             '<meta name="viewport" content="width=device-width, initial-scale=1">',
             '<title>BankingCraft — third-party licenses</title>',
             '<style>body{font-family:system-ui,sans-serif;max-width:960px;margin:2rem auto;'
             'padding:0 1rem;line-height:1.5}pre{white-space:pre-wrap;overflow-wrap:anywhere;'
             'padding:1rem;background:#f4f4f4}td,th{text-align:left;padding:.3rem 1rem .3rem 0}'
             '@media(prefers-color-scheme:dark){body{background:#171b24;color:#eee}'
             'a{color:#9cc6ff}pre{background:#242c38}}</style>',
             '<h1>BankingCraft — third-party licenses</h1>',
             '<p>Generated with cargo-about 0.9.3 from the locked dependency graph. '
             'Includes enabled dependencies across all targets, plus build and test tools; '
             'not every listed package is shipped in every binary. Original copyright notices '
             'that were omitted from crate archives are included under '
             '<a href="upstream/README.md">upstream notices</a>. See also '
             '<a href="../THIRD_PARTY_NOTICES.md">fonts, native libraries and bank data</a>.</p>',
             '<h2>Packages</h2><table><tr><th>Package</th><th>Version</th>'
             '<th>Declared license</th><th>Upstream authors</th></tr>']
    for item in report["crates"]:
        package = item["package"]
        name, version = escape(package["name"]), escape(package["version"])
        url = f"https://crates.io/crates/{name}/{version}"
        label = f'<a href="{url}">{name}</a>' if package["source"] else name
        authors = escape(", ".join(package["authors"]))
        parts.append(f'<tr><td>{label}</td><td>{version}</td>'
                     f'<td>{escape(item["license"])}</td><td>{authors}</td></tr>')
    parts.append('</table><h2>License texts and notices</h2>')
    for index, license in enumerate(report["licenses"]):
        names = ", ".join(f"{u['crate']['name']} {u['crate']['version']}" for u in license["used_by"])
        parts.append(f'<section id="license-{index}"><h3>{escape(license["name"])}</h3>'
                     f'<p>{escape(names)}</p><pre>{escape(license["text"])}</pre></section>')
    parts.append('</html>\n')
    (BUNDLE / "rust-dependencies.html").write_text("\n".join(parts))

    lines = ["# Original upstream copyright notices", "",
             "These supplement crates whose packaged license metadata omitted the original copyright text.", ""]
    for entry in supplements:
        relative = Path(entry["file"]).relative_to("licenses/upstream")
        lines.append(f"- {', '.join(entry['packages'])}: [{relative}]({relative}) "
                     f"([pinned upstream source]({entry['source']})).")
    (BUNDLE / "upstream/README.md").write_text("\n".join(lines) + "\n")

    inputs = [ROOT / "Cargo.lock", ROOT / "about.toml", Path(__file__).resolve(),
              ROOT / "LICENSE", ROOT / "THIRD_PARTY_NOTICES.md", ROOT / "app/data/banks.tsv",
              ROOT / "native/dependencies.json"]
    inputs.extend(ROOT / p for p in ("Cargo.toml", "app/Cargo.toml"))
    inputs.extend(sorted((ROOT / "crates").glob("*/Cargo.toml")))
    files = inputs + [p for p in sorted(BUNDLE.rglob("*")) if p.is_file() and p.name != "manifest.json"]
    manifest = {
        "generator": "cargo-about 0.9.3; scripts/update-licenses.py",
        "rustc": rust_version.splitlines()[0],
        "dependency_count": sum(bool(c["package"]["source"]) for c in report["crates"]),
        "sha256": {str(p.relative_to(ROOT)): digest(p) for p in files},
    }
    (BUNDLE / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    check()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--cargo-about", default=os.environ.get("CARGO_ABOUT", "cargo-about"))
    args = parser.parse_args()
    if args.check:
        check()
    else:
        generate(args.cargo_about)
