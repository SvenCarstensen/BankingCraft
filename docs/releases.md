# Desktop releases

The [Release packages workflow](https://github.com/SvenCarstensen/BankingCraft/actions/workflows/release.yml)
builds the application on GitHub-hosted runners. Local machines are not used to compile release binaries.
All four native builds must pass the license check, workspace tests, Clippy and package startup checks
before a tagged release can be published. No bank credentials or local FinTS test configuration are used.

## Downloads and installation

Get packages from [GitHub Releases](https://github.com/SvenCarstensen/BankingCraft/releases).
While the repository is private, access requires permission to that repository.

| Asset | Target and installation |
| --- | --- |
| `BankingCraft-VERSION-linux-x86_64.AppImage` | x64 Linux, built on Ubuntu 22.04 (glibc 2.35 or newer). Mark executable and launch from a graphical desktop. |
| `BankingCraft-VERSION-windows-x86_64-setup.exe` | Windows 10/11 x64. Per-user installer with Start menu entry, optional desktop shortcut and uninstaller; no administrator rights required. |
| `BankingCraft-VERSION-windows-x86_64-portable.zip` | Windows x64 executable, runtime libraries and notices. Extract the entire folder and run `BankingCraft.exe`. |
| `BankingCraft-VERSION-macos-aarch64.dmg` | Apple Silicon Mac. Open the image and drag BankingCraft to Applications. |
| `BankingCraft-VERSION-macos-x86_64.dmg` | Intel Mac. Open the image and drag BankingCraft to Applications. |
| `BankingCraft-VERSION-source.tar.gz` | Matching committed source, locked Rust dependency sources in `vendor/`, build/package scripts and source/toolchain identifiers. |
| `SHA256SUMS.txt` | SHA-256 checksums for all six assets. |

The macOS deployment target is 11.0. Both DMGs contain an ad-hoc signed app, but are **not
Apple Developer signed or notarized**. Windows packages are **not Authenticode signed**.
Operating-system trust warnings are expected for this first alpha release.
This pipeline does not claim interactive compatibility with every supported OS or reader.

For Linux, a working X11 or Wayland desktop and system graphics drivers are required.
If FUSE is unavailable, run `./BankingCraft-VERSION-linux-x86_64.AppImage --appimage-extract-and-run`.
Smart-card access additionally needs the system PC/SC service and a compatible reader driver;
the AppImage does not install or start system services. PIN/TAN does not require a reader.
On Windows, the installer and portable ZIP include the Visual C++ runtime from the build runner.
The application uses OpenGL and needs a compatible graphics driver on Windows and Linux.
Uninstalling the application does not delete banking vaults or user configuration.

The existing [alpha limitations](../README.md#development-status-and-known-limitations) apply,
including pending FinTS product registration. Packaged startup tests do not contact a bank.

## Creating the next version

1. Update the workspace version in `Cargo.toml`, the five workspace package entries in
   `Cargo.lock`, and both `CFBundleShortVersionString` and `CFBundleVersion` in `macos/Info.plist`.
2. Refresh and review the license bundle using `scripts/update-licenses.py`, and update
   `docs/release-notes.md`. Run `python3 scripts/release-version.py` to check version consistency.
3. Commit and push the changes. Run **Release packages → Run workflow** on the branch to
   validate and download packages without creating a release.
4. After a successful run, create and push an annotated tag matching the version, for example
   `git tag -a v0.0.11 -m 'BankingCraft 0.0.11'` and `git push origin v0.0.11`.

Alternatively, run the workflow on `master` with **publish** checked. This builds and tests the
current version, then creates its tag and publishes the release only after every check passes.
This avoids a second build of the same commit. Manual publication is restricted to `master`;
an existing tag pointing to a different commit is rejected.

A version tag starts all four builds and the source archive job. After all jobs succeed,
the workflow validates the complete asset set, computes checksums, uploads the files to a
draft and publishes it as a **prerelease**. The first release is `v0.0.9`.
By default, manual workflow runs on a branch produce Actions artifacts only; manual runs on a version
tag follow the same publication rules as a tag push. Artifacts are retained for seven days;
published release assets remain attached to the release.

Published releases are never overwritten. If an upload fails, rerun failed jobs to finish the
draft. For changes to released application code, use a new version and tag.

## Packaging and source

`scripts/package-appimage.sh` uses versioned linuxdeploy/appimagetool/runtime downloads with
SHA-256 verification. It collects shared-library copyright notices alongside the reviewed
Rust notices and editable bank directory. The Linux package needs the host's graphics driver
and PC/SC service. Packaging-tool pins must be updated deliberately.

`scripts/package-release.py` packages already compiled binaries on Windows and macOS.
The Windows installer definition is `packaging/windows/BankingCraft.iss`.
The DMG check verifies its filesystem, bundle metadata, ad-hoc signature and system-only
dynamic-library links. Startup checks run the packaged executables for twelve seconds;
Windows also verifies silent installation and uninstallation.
If the hosted Windows VM cannot provide an OpenGL context, the startup test retries with a
checksum-verified Mesa llvmpipe driver. That driver is used only in the temporary test installation
and is not included in the installer or portable ZIP. Other startup failures still fail the job.

Extract the matching source archive and follow the platform prerequisites in the README.
Its `.cargo/config.toml` points to the bundled Rust sources, so `cargo build --release --locked
--offline -p bankingcraft` does not need to download crates. Rust 1.99.1 and the platform SDK/system
development libraries must still be installed. Source and binary archives identify the same commit.
