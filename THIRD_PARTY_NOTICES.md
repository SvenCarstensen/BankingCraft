# Third-party notices

BankingCraft is licensed under GPL-3.0-only; see [LICENSE](LICENSE). Dependencies,
embedded fonts and bank-directory data retain the licenses and copyright notices
of their respective authors. These notices do not relicense those components.

## Rust dependencies

[licenses/rust-dependencies.html](licenses/rust-dependencies.html) contains the
package versions, declared licenses and full license texts for the enabled
dependency graph in `Cargo.lock`, including transitive, build, test and
platform-specific dependencies. This intentionally includes more packages than
any one desktop binary. Where a dependency offers a choice, the generator
prefers MIT; conjunctive (`AND`) requirements remain included.

[Original upstream notices](licenses/upstream/README.md) supplement crates that
omit their original license/copyright file from the published crate archive.
Each entry records the exact upstream revision and a checksum. Where upstream
provides only a licensing declaration or author attribution, that original
declaration is preserved rather than inventing a copyright statement.

The statically linked Rust standard library is outside Cargo's dependency graph.
Its [copyright and third-party notices](licenses/rust-runtime/COPYRIGHT-library.html)
and the [MIT](licenses/rust-runtime/LICENSE-MIT) / [Apache-2.0](licenses/rust-runtime/LICENSE-APACHE)
license texts are included separately, with the exact
[toolchain version](licenses/rust-runtime/toolchain.txt). Regenerate these notices
when building a release with a different Rust toolchain.
The original notices from the toolchain's vendored library sources and LLVM
unwinder are preserved in [SOURCE-NOTICES.txt](licenses/rust-runtime/SOURCE-NOTICES.txt).
This includes standard-library build/test components as a conservative superset.

## Embedded fonts

The following unmodified fallback fonts are bundled through `epaint_default_fonts`:

| Font | Original license and attribution |
| --- | --- |
| Ubuntu Light | [Ubuntu Font Licence 1.0](licenses/fonts/UFL.txt) |
| Noto Emoji | [SIL Open Font License 1.1](licenses/fonts/OFL.txt) |
| Hack Regular | [MIT, public-domain DejaVu contributions and Bitstream Vera notice](licenses/fonts/Hack-Regular.txt) |
| Emoji Icon Font | [MIT notice](licenses/fonts/emoji-icon-font-mit-license.txt) |

[FONT-NOTICES.txt](licenses/fonts/FONT-NOTICES.txt) additionally preserves the
copyright and license strings embedded in the original font files. The font
licenses apply to the fonts, not to documents produced with BankingCraft.
Helvetica, Arial, Segoe UI and other installed system fonts are read from the
user's computer; those font files are not distributed by BankingCraft.

## Native components

Linux Wayland window decorations use Crossfont and FreeType. Portions of this
software are copyright © 1996–2023 The FreeType Project
([www.freetype.org](https://www.freetype.org/)). All rights reserved.
The FreeType License option is used; its [full text](licenses/native/freetype/FTL.TXT),
[upstream license overview](licenses/native/freetype/LICENSE.TXT), and the
[BDF](licenses/native/freetype/BDF.txt) and [PCF](licenses/native/freetype/PCF.txt)
notices are included. This covers the vendored FreeType source in `freetype-sys`;
Linux builds normally link to the system's FreeType installation.

The original composite notices for `ring` are preserved in
[licenses/native/ring](licenses/native/ring), including its BoringSSL and Fiat
components. [Plasma Wayland protocol notices](licenses/native/plasma-protocols.txt)
retain the per-protocol copyright declarations in addition to the license text
in the Rust dependency report. Operating-system frameworks, PC/SC services and
card-reader drivers installed separately retain their own licenses.

## Bank directory from HBCI4Java

`app/data/banks.tsv` is derived from
[HBCI4Java's blz.properties](https://github.com/hbci4j/hbci4java/blob/master/src/main/resources/blz.properties).
Credit belongs to the HBCI4Java authors and contributors. The imported directory
is recorded as an October 2026 snapshot; the original import did not record a
commit identifier. The unchanged upstream LGPL text was retrieved from reference
revision [`efce5c882a4c04d6d6b6f6bd9073707e253d3754`](https://github.com/hbci4j/hbci4java/blob/efce5c882a4c04d6d6b6f6bd9073707e253d3754/LICENSE).
That reference is not a claim that the original import came from that exact commit.

The directory retains the **GNU Lesser General Public License, version 2.1**;
the [complete LGPL-2.1 text](licenses/bank-directory/LGPL-2.1.txt) accompanies it.
BankingCraft converted selected fields to UTF-8 TSV, retained entries with
banking endpoints, sorted them for lookup and updated selected bank names and
endpoints. The corrections of 2026-11-13 are documented in
[docs/bank-compatibility.md](docs/bank-compatibility.md).
The editable corresponding data source is [app/data/banks.tsv](app/data/banks.tsv);
it is included with the macOS license bundle. The directory can be modified in
that source file and incorporated by rebuilding BankingCraft.

## AqBanking and Libchipcard runtime

The application now delegates banking to **AqBanking 6.9.2 (GPL-2.0-or-later)**,
using **Gwenhywfar 5.14.3 (LGPL-2.1-or-later)** and **Libchipcard 5.1.6
(LGPL-2.1-or-later, with the upstream OpenSSL exception)**. Original COPYING
and authorship files are retained in `licenses/native/` under each project name.
Pinned source URLs and SHA-256 hashes are in `native/dependencies.json`.
BankingCraft's changes are applied explicitly by `scripts/build-native.py` and
described in [the native runtime documentation](docs/native-banking.md).

Packages include the actual transitive native library inventory and license
notices in `native/`. macOS uses Homebrew metadata, Windows MSYS2 package
metadata, and Linux Debian/Ubuntu package metadata. Sources for the three pinned
projects accompany the source archive. Before distributing binaries, collect the
matching corresponding sources and patches for bundled GPL/LGPL transitive
libraries as well; the inventory and notices alone do not satisfy that obligation.
No new release has been published as part of this migration.

## Maintaining and distributing these notices

Keep this file, `LICENSE`, the complete `licenses/` directory and the editable
bank directory with distributed application packages. The macOS packaging script
copies them into `Contents/Resources` and checks for stale notices before building.

After changing dependencies, regenerate and review the bundle:

```sh
cargo install cargo-about --version 0.9.3 --locked --features cli
python3 scripts/update-licenses.py
python3 scripts/update-licenses.py --check
```

Regeneration also requires the Rust toolchain's source and documentation components
(`rust-src` and `rust-docs` when using rustup). Offline verification uses the checked-in files.

The offline check verifies the lockfile, manifests and included notice files.
New dependencies whose crate archive omits its copyright notice require a reviewed
upstream supplement in `licenses/upstream/sources.json`. These checks validate
the collected files; they are not a legal compatibility determination.

License notices do not replace the corresponding-source obligations when
distributing GPL/LGPL binaries. Publish the matching BankingCraft source revision,
including modifications and build instructions, and the required dependency
sources with a release (for example, a source archive prepared with
`cargo vendor --locked`). A link to a moving development branch is not a substitute
for the exact source used to build a distributed binary.
