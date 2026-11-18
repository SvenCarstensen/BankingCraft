# BankingCraft 0.2.1

Version 0.2.1 fixes the FinTS implementation behind the instant-transfer switch and
adds the missing scheduled-transfer operation. It remains an **early-alpha release
for evaluation and development**. The new payment paths have been verified with
synthetic data only; no real payment was submitted for this release's tests.

## Instant transfers

- Submit an instant transfer through HKIPZ 1/2 with `INST` in the payment XML.
  Previously, the switch changed an AqBanking subtype while the backend still
  selected a standard SEPA transfer. There is now no fallback or permitted
  conversion from instant to standard SEPA.
- Negotiate the bank's supported payment format and require the account's payment,
  status-query and recipient-verification capabilities. Continue to use the existing
  VoP and BestSign approval flow.
- Distinguish completed, pending, rejected and unknown outcomes. Persist the bank
  reference and earliest next query time inside the encrypted payment order before
  querying status. Bank waiting intervals and a bounded number of HKIPS requests
  prevent unbounded polling; status queries never send a second payment.
- Add **Status prüfen / Check status** to unresolved instant orders. Resume by the
  saved reference after restarting, including when the execution report arrived
  but the worker's final acknowledgement was lost. Cancellation, response loss and
  persistence failures leave orders protected against automatic resubmission.

## Scheduled transfers

- Implement the previously missing HKCSE operation and its HICSE/HICSES responses
  in the pinned AqBanking backend.
- Preserve the requested future execution date in the payment document and enforce
  both bank-supplied lead-time bounds before dispatch.
- Report bank acceptance for later execution separately from settlement. Rejections
  do not become success, and bank notices such as a changed bank-business-day date
  remain in the acceptance message.
- Enable instant and scheduled choices only when the selected account advertises
  the necessary capabilities. A template never silently changes its payment type.

## Downloads and upgrading

- Synchronize the bank connection after upgrading to refresh payment capabilities.
  Verify older uncertain orders against the bank's records instead of submitting
  them again. A previously stored order without a bank reference cannot acquire
  that reference retrospectively from this update.
- Existing encrypted vaults remain readable; encryption, filename and key derivation
  are unchanged. Keep a backup and avoid saving an updated vault with an older app,
  which can discard the new instant-payment status metadata.
- **macOS:** open `BankingCraft-0.2.1-macos-aarch64.dmg` on an Apple Silicon Mac and
  drag BankingCraft to Applications. The app and DMG are Developer ID signed and
  Apple notarized, with stapled tickets. The authorized signer remains
  **Sven Carstensen (Y322FFQ9N6)**; AppCreation GmbH remains the publisher.
- **Windows:** run the x64 installer or extract the complete portable ZIP.
  Windows packages are not Authenticode signed.
- **Linux:** make the x64 AppImage executable and launch it from a graphical desktop.
  It requires glibc 2.35 or newer; use `--appimage-extract-and-run` without FUSE.
- Matching source with locked Rust dependencies, license notices and `SHA256SUMS.txt`
  accompany the five packages. There is no new Intel Mac package; the existing
  [0.1.0 Intel download](https://github.com/SvenCarstensen/BankingCraft/releases/tag/v0.1.0)
  remains unchanged.

See [payment implementation](https://github.com/SvenCarstensen/BankingCraft/blob/v0.2.1/docs/instant-payments.md),
[installation](https://github.com/SvenCarstensen/BankingCraft/blob/v0.2.1/docs/releases.md),
[testing](https://github.com/SvenCarstensen/BankingCraft/blob/v0.2.1/docs/testing.md)
and [earlier releases](https://github.com/SvenCarstensen/BankingCraft/blob/v0.2.1/docs/release-history.md).

## Validation and limitations

Local preparation passed 202 workspace tests (8 ignored), Clippy, a Debug build,
25 release-tooling tests and native offline checks. The payment harness verifies
nine XML variants, FinTS messages, bank capabilities, date boundaries and response
handling using the actual patched backend. It prevents network connections and
substitutes the sending step. Existing VoP/BestSign regressions remain enabled.

Publication requires all three GitHub-hosted platform builds, license checks,
workspace tests, Clippy, native runtime validation and package startup checks to
pass. macOS additionally requires signature verification, notarization, stapled
tickets and Gatekeeper acceptance. The new instant and scheduled paths have no
live-bank acceptance or settlement evidence. Earlier user-reported Postbank transfer
acceptance does not validate these new paths. Existing
[alpha limitations](https://github.com/SvenCarstensen/BankingCraft/blob/v0.2.1/README.md#development-status-and-known-limitations)
continue to apply.
