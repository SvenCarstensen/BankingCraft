# BankingCraft 0.1.1

Version 0.1.1 improves release reliability and makes Apple Silicon the sole macOS
release target. Application features and the encrypted vault format are unchanged
from 0.1.0. This remains an **early-alpha release for evaluation and development**.

## Changes since 0.1.0

- Publish completed releases as regular GitHub releases marked **Latest**, with the
  prerelease flag disabled. Publication still waits for every required package check.
- Retry recognized temporary Apple network errors during credential validation,
  timestamp signing, notarization status requests and ticket stapling. Authentication
  failures, invalid signatures and Apple rejections still fail the build.
- Save each notarization submission ID and poll that submission after network errors.
  An ambiguous upload is not automatically repeated, preventing duplicate submissions.
- Automatically retry confirmed GitHub-hosted runner allocation failures up to twice.
  Application/test failures and manually cancelled runs are not automatically retried.
- Build macOS downloads for **Apple Silicon (ARM64) only**. The existing
  [0.1.0 Intel download](https://github.com/SvenCarstensen/BankingCraft/releases/tag/v0.1.0)
  remains available unchanged; 0.1.1 has no Intel Mac package.

## Downloads and upgrading

- **macOS:** open the `macos-aarch64.dmg` and drag BankingCraft to Applications.
  The app and DMG are Developer ID signed and Apple notarized, with stapled tickets.
  The authorized signer is **Sven Carstensen (Y322FFQ9N6)**; AppCreation GmbH remains
  the app's publisher.
- **Windows:** run the x64 `setup.exe`, or extract the complete portable ZIP.
  Windows packages are not Authenticode signed.
- **Linux:** make the x64 AppImage executable and run it from a graphical desktop.
  It requires glibc 2.35 or newer. If FUSE is unavailable, use `--appimage-extract-and-run`.
- Existing vaults remain compatible. The vault format, filename and key derivation
  have not changed. Reader access still requires the platform's PC/SC service and drivers.
- Matching source with locked Rust dependency sources, license notices and
  `SHA256SUMS.txt` accompany the five downloadable packages.

See [installation and signature verification](https://github.com/SvenCarstensen/BankingCraft/blob/v0.1.1/docs/releases.md),
[Apple signing and retry policy](https://github.com/SvenCarstensen/BankingCraft/blob/v0.1.1/docs/apple-signing.md)
and [earlier release notes](https://github.com/SvenCarstensen/BankingCraft/blob/v0.1.1/docs/release-history.md).

## Validation and limitations

Release jobs run license checks, workspace tests, Clippy, native runtime tests and
packaged startup checks. macOS publication additionally requires strict signature
verification, Apple notarization, stapled tickets and Gatekeeper acceptance.
All release binaries are built on GitHub-hosted runners; a local build service is not
configured. Automated checks use synthetic data and do not contact a bank.

FinTS product registration is pending, PayPal retrieval remains experimental, and
**real payment submission remains unvalidated**. The existing
[alpha limitations](https://github.com/SvenCarstensen/BankingCraft/blob/v0.1.1/README.md#development-status-and-known-limitations)
and platform/bank validation boundaries continue to apply.
