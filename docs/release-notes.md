First packaged alpha release of BankingCraft.

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
