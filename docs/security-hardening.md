# Security hardening and external audit handoff

Status: November 15, 2026. These are implementation changes and internal tests,
not an independent audit. No auditor has been contacted or commissioned. No
PCI-DSS, FAPI, bank certification or security equivalence with GnuCash is claimed.

## Requested changes

| Item | Implementation / status |
|---|---|
| 1. Session locking | Five-minute idle expiry; manual lock; OS lock monitoring; conservative resume/UI-stall detection; focus-loss fallback if no OS monitor is available |
| 2. Release/update signing | Explicitly deferred by the user; existing macOS ad-hoc packaging retained |
| 3. Payment authorization | Fixed full payment summary; control/bidi characters escaped; complete AqBanking VoP dialog; explicit confirmation; individual native job acceptance checked; uncertain outcomes require review |
| 4. Secret lifetime and diagnostics | PIN/TAN fields cleared, caches reset on lock/error, zeroizing vault keys/IPC buffers, no raw native protocol logs, formatted log/panic payloads excluded, conventional Unix core dumps disabled |
| 5. Dependencies | Pinned native sources/tools, actual packaged transitive-library inventory, local SBOM/CVE scan in CI and scheduled dependency workflow |
| 6. Hostile inputs and concurrency | Bounded frames/event batches/images, malformed-input tests, reader response validation, sanitizer adapter tests, process-wide exclusive vault lock with cross-process regression |
| 7. Independent assessment | Scope and reproducible handoff prepared below; actual independent audit remains external and outstanding |

## Threat model and limits

Assets: vault password/key, stored bank PIN, transient TAN, card private keys,
account/transaction data, payment intent and queue status. Untrusted inputs include
bank responses, imported files, TAN images, native IPC and legacy configurations.

Trust boundaries: egui UI to Rust job thread; Rust to native process; native to
bank/TLS or PC/SC driver; authenticated vault to plaintext in-memory data; build
inputs to distributable binaries. A separate worker provides crash isolation and
bounded IPC, **not an OS privilege sandbox**. Malicious code running as the same
user, a compromised OS/driver, or a substituted unsigned application remains
outside the guarantees of this change. Release authenticity is deferred in item 2.

The lock immediately hides financial UI and revokes pending actions. A running
bank job is cancelled and may retain the key until it persists its final state;
then the key and exclusive vault lock are released. An already-sent payment cannot
be recalled by killing the worker. Pending/uncertain orders must be reconciled
with the bank. No automatic resend is permitted.

Lock notifications may only be processed after resume. The system cannot promise
that keys were erased before suspension. Not every copied/native allocation is
zeroized; swap, hibernation and OS crash collection are not fully controlled.
Disk encryption and host security remain separate responsibilities.

macOS notifications are wired to session/screen/sleep events. Windows polls access
to the input desktop; Linux polls logind/screensaver services, with focus-loss
fallback where monitoring is unavailable. Real platform lock/sleep behavior still
needs interactive validation. Idle/resume and UI revocation have automated tests.

TLS certificate errors cannot be accepted through the UI. Card key handling and
bank protocol verification now belong to AqBanking/Libchipcard. Their presence does
not eliminate defects in integration, native dependencies or user authorization.

## Internal evidence

Final local verification after removal of the legacy implementation and the
Postbank/synchronization fixes: 151 workspace tests passed, 8 tests ignored by the default runner (including
subprocess helper tests invoked by their parent tests); Clippy with warnings as
errors passed. The macOS ARM64 release built successfully and the packaged GUI
stayed running for 12 seconds using an empty temporary profile. Native offline,
ASan/UBSan and relocation checks passed. The packaged native scan covered 13
components with zero matches and zero suppressed matches. Postbank PIN/TAN account,
balance and transaction retrieval, including credit cards, completed in separate live
checks on macOS. The user confirmed the account/operation progress display. Other
banks, physical card workflows and Windows/Linux behavior were not exercised locally.

- Workspace tests cover encrypted persistence, exclusive lock/password changes,
  duplicate handling, uncertain queue status, session expiry and prompt revocation.
- C adapter tests use the actual AqBanking VoP dialog with full synthetic bank
  text and acceptance/rejection/cancellation, and reject a certificate error.
- Native tests create a synthetic home, prove no legacy import/config writes and
  reject truncated, duplicate, NUL-containing, oversized and seeded malformed IPC.
- Rust tests cover bounded wire decoding, exact decimal conversion, TAN responses,
  HHD rendering without double encoding and image dimension/allocation limits.
- Import/export, form validation and scoped-refresh regressions remain. The old
  bank simulators and protocol-specific tests were removed with their implementations.
  Refresh tests use a backend double and do not send bank messages.
- ASan/UBSan covers the C adapter. Seeded malformed cases are not a sustained,
  coverage-guided fuzz campaign against all upstream parsers.
- Native SBOM/CVE scans inspect the actual packaged libraries. Known advisory
  matching cannot establish safety or exhaustive coverage.

The RustSec scan reports no known vulnerable, unsound or yanked packages under
its configured gate, but reports three unmaintained-package warnings:
`zbus_names` (RUSTSEC-2026-0371), `zvariant` (RUSTSEC-2026-0370) and
`zvariant_derive` (RUSTSEC-2026-0372). They are also pulled in by Linux accessibility
support; no fixed version is offered by these advisories. These warnings remain
visible and are not suppressed. Track upstream replacement separately.

Commands and native pins: [native-banking.md](native-banking.md). Test only synthetic
fixtures unless the owner explicitly authorizes a live bank/hardware scenario.
Never attach vaults, PINs, TANs, private keys or personal exports to audit reports.

## Independent reviewer handoff

Freeze and identify the reviewed commit, platform/toolchain and package hashes.
Provide matching sources/patches, Cargo.lock, native dependency manifests, SBOM,
scan results and test logs. Review all changes, not only the reused libraries.

1. Trace payment intent from the immutable confirmation through native fields,
   VoP/TAN authorization, individual job status, queue persistence and uncertainty.
   Exercise cancellation before/after sending and around an interrupted save.
2. Review config-manager semantics, native initialization/selection, exact account
   and standing-order binding, response bounds and malformed native snapshots.
3. Review process lifetime, cancellation/session generations, delayed reader/UI
   events, PIN cache invalidation and two-process vault/password-change behavior.
4. Probe hostile imports, bank XML/MT940, image bombs and IPC with instrumentation
   and a coverage-guided fuzz campaign, including upstream libraries.
5. Inspect TLS trust/hostname validation, card-key trust, safe PIN-pad handling and
   representative real bank/card workflows under an agreed test authorization.
6. Inspect every binary/plugin/dependency in Windows, Linux and macOS packages,
   source/license completeness and update/signing work when item 2 is resumed.
7. Deliver an independent dated report identifying auditor, exact scope, findings,
   severity, remediations and retest results. Keep unresolved findings explicit.

Remaining release gates: broader bank/card validation; Windows/Linux CI and hands-on
validation; matching transitive source obligations; independent audit. None of
these is satisfied merely by passing the current offline suite.
