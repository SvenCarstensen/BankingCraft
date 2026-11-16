# BankingCraft 0.1.0

Version 0.1.0 brings the AqBanking/Libchipcard banking backend, category management and
Developer ID signed, Apple-notarized macOS downloads. It remains an **early-alpha prerelease
for evaluation and development**.

## Signed desktop packages

- macOS apps and DMGs for both Apple Silicon and Intel are signed with
  **Developer ID Application: Sven Carstensen (Y322FFQ9N6)**, the authorized personal
  Apple account. AppCreation GmbH remains the app's publisher.
- Both the app and DMG are notarized by Apple and carry stapled tickets. The pipeline
  verifies every embedded native signature, hardened runtime, the configured team,
  Gatekeeper acceptance and startup from the final DMG before publication.
- Linux AppImage and Windows installer/portable ZIP builds use the corrected native
  runtime packaging. Windows uses MSYS2 UCRT64 and a complete bundled runtime;
  installation, application startup and uninstallation are tested on GitHub Actions.
- Native CI fixes cover library exports, runtime lookup, pkg-config ordering and three
  temporary-object memory leaks. Unix sanitizer checks remain enabled.

## Banking backend and retrieval

- Use AqBanking 6.9.2, Gwenhywfar 5.14.3 and Libchipcard 5.1.6 for FinTS/HBCI and card
  operations, with native configuration stored only in the encrypted vault.
- Repair automatic card-context selection for imported RDH accesses and reuse one
  card session during account, balance and transaction refresh. Cancellation, vault
  locking and failed card reads stop that session.
- Correct Postbank account capabilities, CAMT imports and credit-card retrieval.
  Synchronization shows the current account and operation.
- Use neutral account-import wording and remove the old import suffix from access
  names while preserving bank identity, account history and native state.
- Keep securities depots visible without treating their absent balance as a missing
  cash balance in group or overall summaries.

## Interface improvements

- Manage categories on a dedicated sidebar page below Bank connections. Create,
  rename or delete built-in and custom categories per vault. Renaming preserves
  assignments and automatic matching; deletion redirects them to a chosen replacement.
  Changes are saved atomically in the encrypted vault.
- Align adjacent overview cards to the taller content after filtering or resizing.
- Place Lock vault last in the sidebar, with a lock icon and consistent hover styling.
- Show the total number of payment-order entries in the navigation badge, including
  sent orders and orders requiring review; hide the badge when the list is empty.

## Downloads and upgrading

- **macOS:** choose `aarch64` for Apple Silicon or `x86_64` for Intel, open the DMG and
  drag BankingCraft to Applications. Version 0.1.0 is the first Developer ID signed
  and notarized release; the older 0.0.10 downloads are unchanged.
- **Windows:** run the x64 `setup.exe`, or extract the complete portable ZIP.
  Windows packages are not Authenticode signed.
- **Linux:** make the x64 AppImage executable and run it from a graphical desktop.
  It requires glibc 2.35 or newer. If FUSE is unavailable, use `--appimage-extract-and-run`.
- Existing vaults remain compatible; the vault format, filename and key derivation
  have not changed. Reader access still requires the platform's PC/SC service and
  compatible drivers.
- The release includes matching source with locked Rust dependency sources, license
  notices and `SHA256SUMS.txt` for all six downloadable packages.

See [installation and signature verification](https://github.com/SvenCarstensen/BankingCraft/blob/v0.1.0/docs/releases.md),
[Apple signing](https://github.com/SvenCarstensen/BankingCraft/blob/v0.1.0/docs/apple-signing.md)
and [earlier release notes](https://github.com/SvenCarstensen/BankingCraft/blob/v0.1.0/docs/release-history.md).

## Validation and limitations

Release jobs run license checks, workspace tests, Clippy, native runtime tests and
packaged startup checks. These automated checks use synthetic data and do not contact a bank.
The validated pre-release pipeline is documented in
[testing](https://github.com/SvenCarstensen/BankingCraft/blob/v0.1.0/docs/testing.md).

Live account, balance and transaction retrieval was exercised on macOS with Postbank
PIN/TAN and a VR Bank Nord RDH card; the latter used one cyberJack PIN entry.
Other banks, operating systems and card/reader combinations require separate validation.
FinTS product registration is pending, PayPal retrieval remains experimental, and
**real payment submission remains unvalidated**. See the
[README limitations](https://github.com/SvenCarstensen/BankingCraft/blob/v0.1.0/README.md#development-status-and-known-limitations).
