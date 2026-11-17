# BankingCraft 0.2.0

Version 0.2.0 improves Postbank BestSign approvals and chip-card synchronization, adds
searchable activity history and transfer templates, and signals reader input and sync
completion with macOS notifications. It remains an **early-alpha release for evaluation
and development**.

## Payments and BestSign

- Select Postbank BestSign push from the bank's supported methods and repair a legacy
  method selection where the bank provides the current push method.
- Automatically check an outstanding BestSign approval when the bank advertises polling,
  using its waiting intervals and attempt limit. Show progress and the result in the same
  dialog. Status checks never submit the transfer again.
- Omit the extra Postbank recipient-verification dialog only when every submitted payee
  has a complete positive match. Missing, incomplete or mismatching results still prompt.
- Fix AqBanking's cleanup of recipient-verification results, including all owned recipient
  strings, with Linux leak-detection coverage.
- Remove bank-accepted orders from the pending order list, including older sent orders.
  Preserve durable submission records and history; uncertain or failed orders remain
  visible for review and are not retried automatically.
- Save named transfer templates per account, including recipient, amount and payment
  reference. Loading a template or repeating a history entry prepares a new draft for
  review and authorization, without reusing its previous execution date or payment ID.

## Synchronization and reader input

- Batch supported balance and transaction requests for up to 16 accounts per chip-card
  connection, retaining each account's date range and capabilities. Reuse the card session
  while AqBanking controls signatures, message limits and pagination.
- Show a dedicated reader PIN dialog with a 30-second countdown. If the reader has not
  reported completed entry by the deadline, stop the worker and synchronization without
  retrying. The PIN is entered only on the reader.
- On macOS, send a local notification with a quiet bundled chime when synchronization
  finishes or reader input is needed. Respect notification permission and sound settings,
  avoid repeated countdown alerts and clear completed reader prompts. Notifications
  contain no account or payment details. Windows and Linux retain in-app prompts.

## History and interface

- Record bank jobs, payment outcomes, local changes, imports/exports and session actions
  in the encrypted vault. Search entries, filter by message type and sort by type or time.
- Wrap history text fully without truncation, including long action titles and transfer
  details. Clicking a transfer offers a reviewed repeat workflow.
- Import existing accepted orders into history with an explicit note when the original
  confirmation time is unknown; other actions from before this version cannot be recovered.
- Use **Konten synchronisieren** consistently in German and **Synchronize accounts** in
  English, including older history labels. Update French and Spanish translations as well.

## FinTS identity and local development

Normal builds use BankingCraft's own 25-character registration exclusively in HKVVB's
product-designation field. Legacy vault settings cannot replace it; activation may still
vary by bank. Temporary machine-local test configuration is not included in the release.

Local development builds remain Debug builds without Developer ID signing, notarization
or Apple uploads. Explicitly requested public releases use the existing signed GitHub
Actions distribution pipeline.

## Downloads and upgrading

- **macOS:** open `BankingCraft-0.2.0-macos-aarch64.dmg` on an Apple Silicon Mac and drag
  BankingCraft to Applications. The app and DMG are Developer ID signed and Apple notarized,
  with stapled tickets. The authorized signer is **Sven Carstensen (Y322FFQ9N6)**;
  AppCreation GmbH remains the publisher. This release has no Intel Mac package.
- **Windows:** run the x64 `setup.exe`, or extract the complete portable ZIP.
  Windows packages are not Authenticode signed.
- **Linux:** make the x64 AppImage executable and run it from a graphical desktop.
  It requires glibc 2.35 or newer; use `--appimage-extract-and-run` if FUSE is unavailable.
- Existing vaults can be opened with 0.2.0. The encryption format, filename and key
  derivation are unchanged. History and templates are new encrypted data; older versions
  do not understand these fields, so avoid saving an updated vault with an older app.
- Matching source with locked Rust dependency sources, license notices and
  `SHA256SUMS.txt` accompany the five downloadable packages.

See [installation and verification](https://github.com/SvenCarstensen/BankingCraft/blob/v0.2.0/docs/releases.md),
[testing](https://github.com/SvenCarstensen/BankingCraft/blob/v0.2.0/docs/testing.md)
and [earlier releases](https://github.com/SvenCarstensen/BankingCraft/blob/v0.2.0/docs/release-history.md).

## Validation and limitations

Publication requires all three GitHub-hosted platform builds to pass license checks,
workspace tests, Clippy, native runtime validation and packaged startup checks. macOS
also requires signature verification, notarization, stapled tickets and Gatekeeper acceptance.
Automated tests use synthetic data and do not contact a bank or submit a real payment.

Local testing includes user-reported acceptance of a Postbank transfer and earlier Postbank
and VR Bank account retrieval. This is not independent confirmation of settlement or broad
bank/reader compatibility. The batched VR Bank speed improvement has been verified through
reduced synthetic dispatch counts, not a measured live-bank timing comparison. PayPal remains
experimental. The [alpha limitations](https://github.com/SvenCarstensen/BankingCraft/blob/v0.2.0/README.md#development-status-and-known-limitations)
continue to apply.
