# BankingCraft: RDH review and corrections

## Current status — November 14, 2026

**BankingCraft has not been established as equivalent to GnuCash/AqBanking or
libchipcard.** The user confirmed successful VR Bank Nord RDH-7 retrieval on
November 13 after the TCP-dialog correction described below. Postbank PIN/TAN
retrieval is also confirmed in the tested setup. RDH-9/10, other card/reader
combinations and real payment submission remain unverified live.

The sections below preserve the investigation's chronology. Statements that a live
test was still pending, earlier test counts and old UI labels describe their dated
stage, not the current release. The current UI uses egui/eframe 0.36 and the action
is **Synchronize transactions**. Current validation is documented in [Testing](testing.md).
As of November 17, 2026, normal builds use BankingCraft's own registered identity
exclusively in HKVVB's product designation. At the user's request, the temporary local
installation retains the explicit `local-fints-test` feature and external ID file;
missing or invalid local values stop the operation instead of falling back.

## Initial review — November 12, 2026

The review covered the existing source tree, including uncommitted work, which was
preserved. The project, Git history and local changes were moved from the earlier
development checkout to the current repository location. SHA-256 hashes of 46 source
files matched before and after the move.

The core for previously initialized RDH-7/9/10 smart cards was present, but the initial
implementation had response-validation vulnerabilities and card-protocol errors.
These were corrected. A successful real-bank retrieval was still pending at this stage.

GnuCash does not implement RDH itself: it integrates AqBanking. AqHBCI handles dialogs
and message cryptography, Gwenhywfar supplies cryptographic/token abstractions, and
libchipcard provides ZKA smart-card access.

## References and comparison

- [GnuCash: AqBanking](https://code.gnucash.org/wiki/AqBanking).
- AqBanking commit `33504e378993d9cdb6702bd90c7039ef378fe31f` (September 24, 2026):
  [job_crypt.c](https://github.com/aqbanking/aqbanking/blob/33504e378993d9cdb6702bd90c7039ef378fe31f/src/libs/plugins/backends/aqhbci/joblayer/job_crypt.c),
  [msgcrypt_rxh_common.c](https://github.com/aqbanking/aqbanking/blob/33504e378993d9cdb6702bd90c7039ef378fe31f/src/libs/plugins/backends/aqhbci/msglayer/msgcrypt_rxh_common.c),
  [msgcrypt_rxh_verify.c](https://github.com/aqbanking/aqbanking/blob/33504e378993d9cdb6702bd90c7039ef378fe31f/src/libs/plugins/backends/aqhbci/msglayer/msgcrypt_rxh_verify.c).
- libchipcard commit `c0d0f19d36674baf06032731ea6b1830aa31b30f` (April 11, 2025):
  [zkacard.c](https://github.com/aqbanking/libchipcard/blob/c0d0f19d36674baf06032731ea6b1830aa31b30f/src/ct/zkacard/zkacard.c),
  [ZKA card format](https://github.com/aqbanking/libchipcard/blob/c0d0f19d36674baf06032731ea6b1830aa31b30f/src/libchipcard/cards/zkacard/zkacard-app.xml).

Shared mechanisms include double SHA-256 hashing, RSA-PSS, two-key 3DES-CBC,
RSA-PKCS#1-v1.5 key transport for RDH-7/9 and mode 2 for RDH-10. ZKA uses MSE
algorithm 46 for signing, 1A for decryption, EF_KEYD/EF_NOTEPAD and reader-based PIN
entry. Matching source behavior does not itself prove interoperability.
AqBanking checks expected encryption/signatures and offers an explicit user decision
for certain unsigned responses. BankingCraft strictly rejects such personal RDH
responses; anonymous key requests are handled separately.

## Initial findings addressed

| Initial finding | Correction and evidence |
|---|---|
| Unsigned plaintext/encrypted RDH responses and responses without a bank signing key were accepted | Require complete encryption and a valid bank signature; five original failures were reproduced offline and covered by regressions. |
| Plaintext signatures, HNSHK/HNSHA control references and unprotected extra segments were insufficiently checked | Validate signature structure, profile, algorithms and control references; reject plaintext in personal RDH dialogs. |
| A reply could be assigned to the wrong dialog/message | Validate length, header/trailer, dialog ID and message number. |
| A failed card-record lookup fell back to the first connection | Require an exact selected record, bank code and user ID; no silent fallback. |
| EF_KEYD was interpreted as a modulus rather than a length description; public key slots could be selected | Select private slots, parse D0 descriptors and reject missing/invalid lengths; 2048/3072-bit fixtures. |
| Extended APDU Le 256 was encoded as 0000, meaning 65536 | Encode 0100 for 256, support extended Le with short payloads, and check signature length against the card key. |
| Stale signature counters and undersized software PINs could be used | Read the counter immediately before signing, abort on read failure, and enforce minimum PIN length. |
| Unbounded TCP/HTTP replies, raw debug messages and insecure PIN/TAN HTTP | Enforce size/time limits and HTTPS without redirects; log segment identifiers only. |
| Tampered binary lengths/Unicode date fields could cause panics | Use bounded, checked indexing; parser and flicker-challenge regressions. |
| Invalid CAMT amounts became zero/default dates; parser errors/page limits were hidden | Propagate errors and never report incomplete retrieval as complete success. |
| Payment amounts were silently rounded and field/date limits were missing | Validate positive amounts with at most two decimal places, SEPA field lengths and standing-order dates. |
| Renaming could make SEPA IDs too long | Generate short unique BC IDs within the 35-character limit. |
| Password changes could leave disk and memory inconsistent | Re-encrypt saved PINs, atomically save a candidate, then replace in-memory state. |
| Temporary vault files lacked exclusive/private creation | Random temporary name, `create_new`, Unix 0600, `sync_all` and atomic rename. |
| Double-clicks could start parallel bank jobs; a failed list refresh could encourage duplicate standing-order creation | Reserve jobs synchronously and report successful creation separately from failed list refresh. |
| Equal account numbers with different subaccounts could be merged | Include subaccount and bank code when matching account numbers. |

## Scope limits

- No software key files, initial INI registration, key changes/revocation or complete
  certificate workflows. AqBanking supports a broader range of profiles/media, though
  not every conceivable security class.
- New RSA tests used a simulated card/bank. Some messages were generated with the
  same builder on both sides, which can conceal shared errors.
- RDH-7 had a complete simulated-dialog test at this stage. The later live confirmation
  applies only to the specific RDH-7 setup; RDH-9/10 and other hardware still need tests.
- DDV was not extended to provide equivalent bank-signature verification.
- CAMT aggregate entries with multiple `TxDtls` are not fully split into individual
  transactions. This is not a claim of complete CAMT support.
- No independent comprehensive security audit or guarantee of correctness.
- Real retrieval requires a supported initialized smart card, PIN approval and a valid
  registered FinTS product ID. Bank X's product ID was not copied.

## Bank X connection import

An RDH-7 connection with eight accounts was prepared from visible account-folder
settings. Account data was kept only in a private local import file outside Git.
The importer validates connections, adds them after unlocking the vault, backs up the
previous encrypted vault and saves atomically. Repeating an import creates no duplicates;
existing bank keys and connection settings are not replaced. PINs, private keys,
historical transactions and old balances are not imported.

The user confirmed eight additional accounts in the unlocked app, bringing the total
to 13. A real card test was skipped at that point because the reader was disconnected,
at the user's request.

## Rename and icon

The product, Cargo binary, UI, bundle and exports were renamed BankingCraft. The vault
filename, location, file magic and key derivation remain compatible. The B icon was
edited using ImageGen; its source is `macos/bankingcraft-icon.png`. The original K icon
was retained as the starting image.

Final image-edit prompt, using a reference image and transparent background:

> Use case: precise-object-edit. Edit target: BankingCraft B app icon. Clean the alpha edge only: remove ALL stray blue, cyan, purple pixels and artifacts OUTSIDE the rounded blue square. Exterior must be perfectly transparent. Make edge antialiasing clean and smooth; preserve centered white B, existing gradient and rounded square proportions. Flat crisp vector-like finish with no grain or texture. Deliver a square 1024x1024 macOS icon with transparent margins.

The generated raster is 1280×1280 pixels; bundle creation uses `sips` to scale it to
macOS icon sizes.

## Initial verification

`cargo test --workspace --locked` covers local parsers, cryptography, the simulated
RDH dialog, repeatable/validated/encrypted imports, password changes and rollback.
Release and macOS-bundle checks supplement those tests. A successful local build or
import is not evidence of a successful bank request.

### Follow-up after Postbank retrieval

A live Postbank response omitted CAMT booking/value dates. Both are optional, so
missing dates must not abort retrieval. They are now stored explicitly as unknown
and displayed as **not provided**; CSV/XML exports do not invent dates. An internal
report/retrieval date is only a sort key. Non-empty invalid dates remain errors.
HICAZ pending entries remain pending even without an XML status. Pending/undated
entries do not enter monthly analytics.

Synthetic mixed BOOK/PDNG fixtures and date/time values cover the change. The next
real Postbank request completed without the date error, reporting six accounts checked
and no new transactions. User-defined account groups were also added: assignment,
rename and removal are atomically stored; removing a group does not delete accounts.
Serialization, older vaults without groups and unique assignment are tested.

At this stage:

- 33 tests passed: eight app, four card-protocol and 21 FinTS tests, plus doc tests.
- Clippy with `-D warnings` and the locked release build passed; the macOS plist and
  ad-hoc bundle signature were checked.
- The app opened and reused the existing vault successfully.
- Bank X import and groups were confirmed in the UI: five Postbank accounts in a private
  group and eight VR Bank accounts in a separate group.
- The bank supplied six Postbank account records, while the local group contained five
  accounts; bank-record counts and local group counts are distinct.
- Live Postbank retrieval succeeded after the CAMT date fix. No real card retrieval or
  payment was performed during this stage.

### Overview, account ordering and standing-order retrieval

The overview initially showed group totals based on booked balances, with currencies
kept separate, partial totals labelled and missing balances shown as unknown. The action
was then labelled **Fetch transactions**. Since November 14 it is **Synchronize
transactions**, and primary bank balances include pending entries alongside booked totals.

Drag-and-drop ordering is saved atomically in the encrypted vault and survives refreshes
and restarts. Stable account IDs preserve group membership and the selected source
account in transaction/payment forms.

Standing-order retrieval previously hardcoded HKCDB version 1. It now negotiates the
highest mutually supported version, 1 or 2, and an advertised pain.001 format. HICDB
version 2 includes execution time before the final date; the parser reads them separately.
Bank failures no longer produce a misleading zero-orders success, and affected local
orders remain intact. Field-order reference:
[hbci4java hbci-300.xml at 7482dcdb](https://github.com/hbci4j/hbci4java/blob/7482dcdb85b1e6e879501f87e23c3264adc393c3/src/main/resources/hbci-300.xml).

Verification then comprised 38 passing tests: 11 app, four card-protocol and 23 FinTS,
including totals, ordering/persistence, paginated version 1/2 standing-order replies,
execution time and rejection of incompatible formats/versions. Clippy passed.

The corrected real Postbank request loaded two standing orders, both visible in the
app. Group totals and the action label were checked. Mouse-driven reordering survived
a restart; the original order was restored afterward. The reader test was still deferred.

## PIN correction — November 13, 2026

PIN-free diagnostics on a connected cyberJack ecom_a identified a ZKA/SECCOS card
with global PIN reference 3 and ASCII format. The old VERIFY buffer always contained
eight spaces regardless of PIN length. The generic libchipcard PC/SC path instead
passes only the four-byte command header for ASCII; the reader supplies Lc and the
actual PIN. The old template matched the special SCM-X32 path, not the generic path.
This plausibly explained rejection of a correct PIN but did not yet prove a successful
PIN attempt.

BankingCraft now uses the generic ASCII template. Software PINs also use their actual
length; FPIN2 remains separately encoded. EF_PWDD must identify the access PIN
unambiguously. The app does not try other references/encodings or repeat a failed,
cancelled or uncertain attempt within a card connection. EF_FBZ provides the retry
counter without verifying a PIN; diagnostics display it. Zero attempts suppress VERIFY.

Reference: [libchipcard generic_pcsc/card.xml](https://github.com/aqbanking/libchipcard/blob/c0d0f19d36674baf06032731ea6b1830aa31b30f/data/drivers/posix/generic_pcsc/card.xml).
Additional regressions check command bytes and error paths with synthetic PINs only.

All 140 workspace tests and Clippy passed. The release was installed and started.
However, the final PIN-free check found no available reader, so the actual retry
counter could not be read. No PIN verification was attempted; real-card confirmation
and successful VR Bank retrieval were still pending at that stage.

## Key detection — November 13, 2026

VR Bank retrieval reported that key #2 was not on the card. Read-only EF_KEYD diagnostics
showed signing key #2 and decryption key #3 were present, each 248 bytes/1984 bits.
Only authentication key #4 was 256 bytes/2048 bits. The parser had rejected all keys
below 256 bytes, incorrectly treating both required keys as absent.

Detection now accepts 248-byte keys while still requiring private references and valid
length descriptors. A regression using the same metadata structure and a synthetic
counter reproduced the failure. Other tests cover boundaries, public references and
exact APDU lengths. A simulated RDH-7 dialog verifies signing/decryption with 1984-bit
card keys.

The corrected production reader code then detected both 1984-bit keys on the connected
card without PIN verification, signing or bank communication. This alone did not confirm
a complete bank retrieval. All 186 workspace tests and Clippy passed; two manual render
exports were skipped. The release replaced the existing macOS executable, its ad-hoc
signature was checked and the app reached the vault-unlock screen.

## Anonymous bank-key retrieval — November 13, 2026

After key detection was fixed, retrieval failed with codes 9050/9800/9110. The cause
was missing FinTS product registration: Atruvia misleadingly rejected the combined
V/S-key request with the placeholder `BANKINGCRAFT` as an invalid segment composition.
An anonymous BPD request using the same product name returned the explicit code 9078,
meaning the software was not registered as a FinTS product.

At the user's explicit request, another product ID was temporarily used for a local
test. Its value is not documented in the repository. The initial compile-time
`BANKINGCRAFT_DEFAULT_PRODUCT_ID` mechanism was subsequently removed: only an explicit
`local-fints-test` build now reads a private external file at runtime. Normal builds
ignore it. A configured vault ID takes precedence, and the fallback is never saved
to the vault.

With that test configuration, the production FinTS client received both public bank
keys from `fints1.atruvia.de:3000` for public bank code 21763542: V and S, each 2048 bits,
number 7, version 2, with response codes 0020 and 0010. Both HKISA segments must be in
one request, as in the AqBanking/HBCI4Java templates. A single HKISA request returned
only BPD, so the combined request was retained.

This test used synthetic card metadata and an anonymous customer ID. Fingerprint
confirmation was declined: no keys were stored, and no personal login, PIN verification
or signature was requested. Full personal retrieval was still pending.

The original HKISA segment summary is now retained if the subsequent anonymous dialog
end fails with a different error, as happened with code 9800. Retrieval errors identify
the affected bank connection. Regressions cover the combined request, 2048/1984-bit
RDH-7, missing/preserved keys, diagnostics and correct attribution after a partially
successful global refresh.

At this stage, 188 tests and Clippy passed, with two render exports skipped. The local
release and its build UUID/ad-hoc signature were checked, and the app restarted to
the unlock screen. The temporary compile-time test-ID build used during this stage
was later superseded by the runtime-only separation described below.

## Personal RDH retrieval — November 13, 2026

The user-authorized VR Bank request started with an empty outgoing queue. After reader
PIN entry, it failed because the RDH response was not fully encrypted. Security errors
had not saved segment summaries, concealing the underlying bank rejection.

Source comparison found another protocol error: the signing-key version came from
EF_KEYD, but the bank-facing versions in the selected EF_NOTEPAD context are authoritative.
libchipcard also takes versions from that context: `LC_Crypt_TokenZka_GetContext`,
lines 772–788 of the linked `zkacard.c` revision. BankingCraft now uses context signing/
decryption versions while retaining physical slots and lengths. The 2048/1984-bit
RDH-7 tests reproduced the old wrong version and passed after correction.

Pure unencrypted error responses can now contribute only 9xxx codes as explicitly
unauthenticated diagnostics, with count/length limits and control characters removed.
The reply is still discarded: success, business data, system ID and bank parameters
are never accepted. Mixed replies are not interpreted. Security errors also receive
content-free segment summaries; encrypted requests expose only requested segment
names/numbers.

All 190 workspace tests and Clippy passed, with two render exports skipped; live
confirmation of this correction remained pending at that point.

After installation and another PIN approval, HKIDN/HKVVB received an encrypted reply,
but the next failure concerned the message envelope. Diagnostics still showed the
previous reply, so summaries now update before envelope validation. They expose only
numeric lengths/versions/counters and a boolean dialog-ID match, never dialog IDs.
Pure errors with inconsistent envelopes can display unauthenticated 9xxx diagnostics;
all envelope checks remain mandatory. A regression verifies the updated, minimal
summary for a rejected reply with message number 9999. This brought the passing count
to 191, with Clippy also passing.

## Renewed source comparison: card access and TCP dialogs — November 13, 2026

The next live attempt completed personal dialog initialization. HKSPA, message 2, then
received an unencrypted 9800 gateway rejection stating that the customer message contained
invalid data. The reply header used message number 2 but its trailer used 1. Strict
envelope validation was correct; weakening it would not address the cause.

The comparison used the same reference revisions cited above: libchipcard for card
access, and its caller AqBanking/AqHBCI for FinTS messages and connections.

| Area | Reference behavior and BankingCraft finding |
|---|---|
| Card files | MF/EF_ID `0003`, EF_PWDD `0015`, EF_FBZ `0016`, DF_SIG `d27600006601` with EF_KEYD `0013`, and DF_NOTEPAD `d2760000254e500100` with EF_NOTEPAD `a611` match `zkacard-app.xml`. |
| PIN description and reader | PWDD pseudo-OID selects ASCII/FPIN2 and minimum length. Generic PC/SC ASCII passes only the four-byte VERIFY header; FPIN2 has a separate eight-byte block. The ASCII fix matches `generic_pcsc/card.xml`. BankingCraft rejects unknown formats where libchipcard sometimes assumes ASCII. No automatic PIN retry. |
| Private card keys | Private references `84`/`94`, D0 length descriptors and highest physical-version selection match the ZKA path. Previously excluded 248-byte keys are recognized. The record, not the FinTS key number, addresses the card command. |
| Context and versions | Equal signing/encryption key numbers in the notepad identify the RDH profile. Physical keys are #2/#3; bank-facing versions come from EF_NOTEPAD, matching `LC_Crypt_TokenZka_GetContext`. |
| Card identifier | Complete raw EF_ID is used as CID, matching `LC_Crypt_TokenZka__ReadContextList`. Missing customer ID falls back to user ID. |
| Signature counter | Use the 16-bit complement value from KEYD tag `91`, read immediately before signing. Read failure never reuses a stale counter. |
| Signing | MSE `00 22 41 A4` with `84 01 (Record\|80)` and `80 01 46`, followed by INTERNAL AUTHENTICATE `00 88 00 00`, matches `LC_ZkaCard_Sign` and `_isoManageSe`. The host hashes once with SHA-256; the card performs the second hash and RSA-PSS. |
| Decryption | MSE `00 22 41 B8`, algorithm `1A`; PSO DECIPHER `00 2A 80 86` with leading `00`, matching libchipcard. BankingCraft requests the known modulus length as Le rather than the maximum response length. Tests cover 248/256/384 bytes. |
| FinTS signature | HNSHK 4, RDH profile, function 2, role/scope 1, hash `1:6:1`, signature `6:10:19`, and S key with profile number/context version match AqBanking for this security class. |
| FinTS encryption | HNVSK 3, function 4, two-key 3DES-CBC; RSA-PKCS#1-v1.5 transport mode 18 for RDH-7/9 using the bank's public V key. RDH-10 separately uses raw RSA mode 2. |
| Anonymous key request | HKIDN/HKVVB with V and S HKISA segments and user/number/version `999` matches `jobgetkeys.xml`. The real server returned both keys. |
| System ID | Signed/encrypted HKIDN/HKVVB/HKSYN with system ID 0 matches `jobgetsysid.xml`. |
| TCP connection | **Newly identified defect:** AqBanking keeps `dlg->ioLayer` open for the entire dialog. BankingCraft created a new socket on each `send()`, losing the connection-bound dialog. |

### Cause, correction and evidence

[AqBanking `dialog_hbci.c`](https://github.com/aqbanking/aqbanking/blob/33504e378993d9cdb6702bd90c7039ef378fe31f/src/libs/plugins/backends/aqhbci/msglayer/dialog_hbci.c)
creates a socket only when `dlg->ioLayer` does not exist. Subsequent send/receive calls
reuse it until dialog closure.

A PIN-free test using only public/anonymous information confirmed its importance for
Atruvia: HKISA followed by HKEND on the same connection returned 0010 (message accepted)
and 0100 (dialog ended). The earlier HKEND on a new connection returned 9800.

`TcpTransport` now retains its stream across a dialog's messages. `Client::reset_dialog()`
closes it; the next dialog opens a new connection. Ending without an assigned dialog ID
also cleans up the transport. A transport error poisons the connection until an explicit
new dialog. There is no automatic reconnect within a dialog and no replay of a request
with an uncertain outcome. HTTPS behavior and signature/encryption/envelope checks remain.

Three local-socket regressions cover multiple messages on one socket, fragmented replies,
closing/reconnecting at dialog boundaries and preventing retry after a truncated reply.
The second-message test failed before the fix and passed afterward. All 194 workspace
tests and Clippy passed; two manual render exports remained skipped.

### Remaining differences and actual live confirmation

This does not establish full libchipcard equivalence. BankingCraft implements the known
ZKA path, without alternative KEYD layouts D1/D2/D8, card initialization, certificate
workflows or complete vendor-specific reader templates. EF_GD0/EF_SSD are not interpreted
as in libchipcard's general reopen path. PWDD reads only the first eight records,
versus up to 31 in libchipcard; the tested card's unambiguous description is in that range.
The general KEYD/notepad reader stops enumeration for read failures other than 6A83 as
well, so an incomplete list can appear as a missing key/connection. The requested context
and required keys must nevertheless exist; there is no fallback to another bank connection.

The concrete live subject was RDH-7. RDH-9/10 and other hardware combinations are not
validated by this result. The authorized local test build was installed, and its ad-hoc
signature and build UUID `E7E7D670-56DD-3384-98D8-58F1F69BB39D` were checked. A local backup
of the previous app was retained. After the final dialog-end fix, all 194 tests and
Clippy still passed. The app started to the unlock screen. **The user then confirmed
successful personal VR Bank retrieval with the TCP correction.**

## Separating the local test product ID from the repository

At the user's request, the concrete ID value was removed from this report before any
commit contained it. `BANKINGCRAFT_DEFAULT_PRODUCT_ID` is no longer read.

Only the explicitly enabled `local-fints-test` app feature reads a fallback from
`BankingCraft/local-tests/fints-product-id` in the local user's configuration directory.
On macOS this is outside the project under `~/Library/Application Support`; directory
permissions are 0700 and file permissions 0600. The ID is neither embedded in the binary
nor saved in the vault. A configured vault ID takes precedence. Normal builds ignore
the file; sharing a test binary does not share its local configuration.

This development checkout has local `pre-commit`, `commit-msg` and `pre-push` hooks.
A separate private denylist outside the project allows scanning without putting the
value into scripts or versioned files. Hooks inspect staged contents/names, commit
messages and all reachable objects in pushed history, including previously deleted
occurrences and binary data. Hooks and denylist are intentionally machine-local;
this is not a server-side GitHub control. At the time of the original guard check,
nothing had yet been pushed; subsequent authorized pushes use these same guards.

Disposable-repository tests used synthetic denied values to exercise clean content,
staged matches, commit text, later-deleted matches and UTF-16 binary data across chunk
boundaries. The existing history and index passed.

At that stage, 197 workspace tests and Clippy passed both with and without
`local-fints-test`; two render exports remained skipped. A temporary local check without
bank traffic confirmed runtime loading only in the test build and precedence of a
configured vault ID. The normal build also ignored the old environment variable.
The temporary helper was removed afterward. The test ID was absent from all current
versioned/non-ignored files and the release binary. The local test app was installed,
its signature/build UUID `E380DB8B-C372-35B6-961E-FA6A0E0F256C` checked, and startup to the
unlock screen confirmed. No further bank request was needed.
