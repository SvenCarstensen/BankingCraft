# License bundle

Start with [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md).

- [Rust dependencies](rust-dependencies.html): package versions and full license texts.
- [Upstream notices](upstream/README.md): original notices missing from crate archives.
- [Fonts](fonts/): original licenses and embedded copyright declarations.
- [Native components](native/): FreeType, ring and Plasma protocol notices.
- [Rust runtime](rust-runtime/): standard-library licenses and copyright notices.
- [Bank directory](bank-directory/LGPL-2.1.txt): full upstream LGPL-2.1 license.
- [Manifest](manifest.json): dependency count, toolchain and SHA-256 checksums.

The bundle is copied into macOS applications by `macos/build-macos.command`.
Run `python3 scripts/update-licenses.py --check` to verify it offline. To update it,
install cargo-about 0.9.3 with its `cli` feature and run
`python3 scripts/update-licenses.py`; review new upstream declarations and license choices.

Original license texts are kept verbatim. Some upstream projects supply only a
license declaration or author list; those are included as supplied, alongside
the full standard license text. The collection is not a blanket legal clearance.
