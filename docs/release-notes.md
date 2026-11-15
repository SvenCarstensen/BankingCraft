BankingCraft 0.0.10 includes the local interface improvements that were not part of 0.0.9.

- Drag accounts to reorder them or move them between groups, including collapsed and empty
  groups. Dragging near the viewport edge scrolls the list; Escape cancels a drag.
- Collapse and expand account groups independently, retaining their state within each vault
  while navigating, refreshing or renaming groups.
- Highlight negative balances separately for each currency and for booked balances.
- Clear input fields with an inline button, with improved vertical text alignment.
- Optionally add one fictional demo account with sample transactions when creating the first
  vault. The checkbox is enabled by default and can be cleared; subsequent vaults start empty.
- Include translations and regression tests for group dragging, cancellation, encrypted
  persistence and first-vault setup.

Downloads:

- Linux x64 AppImage.
- Windows x64 EXE installer and a portable ZIP containing the executable and runtime libraries.
- Separate macOS DMGs for Apple Silicon and Intel.
- Matching source archive including the locked Rust dependency sources.
- SHA-256 checksums and bundled license notices.

All binaries are built on GitHub Actions. Release jobs run workspace tests, Clippy,
license checks and packaged startup checks; Windows also checks installation and uninstallation.

This is an early alpha for evaluation. FinTS product registration is pending and real payment
submission remains unvalidated. See the README for the tested banks and other limitations.
The macOS apps are ad-hoc signed but not notarized; Windows packages are not Authenticode signed.

For macOS, choose `aarch64` for Apple Silicon or `x86_64` for Intel and drag the app to Applications.
For Windows, choose `setup.exe` to install or extract the complete portable ZIP.
For Linux, make the AppImage executable; glibc 2.35 or newer and a graphical desktop are required.
If FUSE is unavailable, use `--appimage-extract-and-run`. Smart-card readers require the platform's
PC/SC service and compatible drivers.
