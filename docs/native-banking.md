# Native banking runtime

Integration status: November 15, 2026. This change has been tested offline on
macOS Apple Silicon. Postbank credit-card retrieval and the full Postbank account
refresh have also completed in live checks. A VR Bank RDH refresh was exercised
with a cyberJack, reusing one worker through the last account; the tester confirmed
a single reader PIN entry. Other physical-card/reader combinations remain unverified.
Windows/Linux automation is configured; its results are recorded in GitHub Actions.
The local development app has been updated. No release containing this migration
has been published.

## Boundaries and migration

`app/src/jobs.rs` uses `crates/aqbanking` for banking operations. The isolated
`bankingcraft-aqbanking` worker calls AqBanking's banking/provider APIs and its
pinned AqHBCI setup and TAN-selection APIs. Libchipcard crypt tokens handle DDV/ZKA
cards; its chipTAN USB API handles TAN generation. Reader discovery still uses
PC/SC. The application no longer invokes the old custom card diagnostic/APDUs.

The former `fints` and `cardreader` crates have been removed. Their FinTS message
construction, BPD/UPD handling, transports, bank-response parsers, DDV/RDH crypto,
card APDUs, USB TAN and PIN-pad implementations are no longer in the workspace.
`bankingcraft-models` contains only shared data, display helpers and local input
validation. Reader inventory is a small PC/SC enumeration module in the AqBanking
adapter; it does not open cards, exchange APDUs or perform cryptography. Libchipcard
provides the card operations. PayPal remains a separate read-only NVP implementation.

The old optical HHD encoder and matrix container parser were also removed. The
UI renders already prepared AqBanking HHD bytes/images. The offline demo uses a
fixed published sample. Custom FinTS protocol/error trace output was removed.
Native message construction, statement parsing and VoP handling belong to AqBanking.
The application's CSV/camt export remains an output feature for the user's stored
transactions. Its test checks XML directly, without retaining a bank-response parser.

AqBanking settings are an opaque, versioned snapshot inside each access's encrypted
vault record. A custom Gwenhywfar config manager keeps groups in memory. A regular
empty file blocks writes through the native data-directory path. No native secrets
are placed in command-line arguments or plaintext configuration files. The native
process receives only the selected access and requested operations through pipes. Debug builds
can use `.native/helper-path`; release builds require the packaged worker.

The snapshot reader uses Gwenhywfar's `GWEN_PATH_FLAGS_CREATE_GROUP`, matching its
directory config reader. Repeated `updjob` and `tanMethod` groups must remain
separate: reading with default flags alone merges their fields and loses the
associations AqBanking relies on. The internal `snapshot_format=2` marker upgrades
older snapshots once by invalidating initialization and resetting BPD/UPD version
numbers through AqBanking. The next user-requested operation obtains fresh bank
parameters; native account IDs, selected device and the vault's history are kept.
The reader also preserves repeated groups in native standing-order descriptors.

The first connection initializes fresh AqBanking settings. AqBanking chooses bank
statement formats; the former application-level CAMT/MT940 selector was removed. Old BPD/UPD/TAN settings
are not treated as native AqBanking state. Binding the snapshot to the bank/user,
endpoint, security medium and card context prevents reuse with a different access.
Obsolete protocol fields in older vaults are ignored by serde; native snapshots
and the vault's accounts/history remain readable. Backward compatibility with an
older application after saving the new format is not promised.
Existing transaction categories are retained; multiset duplicate matching handles
formatting differences while preserving legitimate identical bookings. Old standing
orders must be fetched again before deletion, to obtain the native bank descriptor.

Imported card accesses can store entry `0` for automatic selection. Before adding
such a user, the adapter reads Libchipcard's context list and resolves a unique
match for the configured bank code and user ID. AqBanking receives the actual
nonzero context ID and the same card's token name. Missing or ambiguous matches
stop setup; explicit entry IDs remain unchanged. Selection is needed only before
the native user exists and may require a separate card PIN confirmation.

Card account refreshes keep one native worker and one Libchipcard token alive for
account discovery, balances and transactions of the same access. AqHBCI's
`nounmount` setup option and AqBanking's provider queue API avoid closing the card
after each read. No PIN is cached by the adapter or added to the snapshot; the
reader still verifies it. Additional card-required authentication remains possible.
The first import may need a separate PIN for automatic card-context selection.

The session accepts only read operations and account/date selectors after startup;
it cannot change its bank identity, reader, medium or snapshot. Payments and card
management run separately. Successful reads export updated snapshots immediately.
Normal completion closes/unmounts the token. Vault locking, cancellation, process
failure and timeout dispose of the worker. A card/bank read failure stops the
refresh instead of restarting authentication for each remaining account; local
unsupported-account capabilities may be skipped without reopening the card.
The application's per-account balance/transaction progress labels remain intact.

PIN/TAN initialization follows AqBanking's setup wizard and
[documented setup order](https://www.aquamaniac.de/rdm/projects/aqbanking/wiki/SetupPinTan):
anonymous bank parameters (including SCA), system-id synchronization, TAN selection,
then accounts and AqBanking's SEPA account-information request (`getaccsepa`/HKSPA),
if the bank advertises it. This lets AqBanking fill in IBAN, BIC and account suffix
before statement jobs are built; the application never guesses these identifiers.
Existing snapshots receive the missing SEPA step once, and account discovery
invalidates that marker. Balance/statement jobs reuse it. Failed SEPA requests stay
visible and stop the access rather than being marked complete or silently retried.
Banks may reject the anonymous step; certificate rejection and user
cancellation still abort it. An initialized snapshot skips setup on later jobs, and
the first account discovery reuses the setup result rather than fetching it twice.
Setup failures stop refresh for that access. Already loaded balances, transactions
and updated native state are retained, without claiming that all accounts were checked.
Native errors include their operation stage and preserve the first distinct warnings
and errors (bounded to eight messages/16 KiB), including those preceding dialog cleanup.

AqBanking's TAN-method requirement controls the device-name prompt. Required names
are entered separately from PIN/TAN, kept exactly (including spaces/case), and saved
through AqBanking inside the encrypted snapshot. Existing initialized snapshots are
checked locally for missing required names. TAN reset edits the bank's cached methods
and device name without requiring a successful login, so a wrong name can be corrected
after incomplete setup. Cancelling an edit preserves the previous configuration.

Accounts enable AqBanking's `preferCamtDownload` preference only when AqBanking's
parsed, account-specific UPD advertises HKCAZ. Bank-wide CAMT support alone does
not authorize it on savings or credit-card accounts. The adapter clears older
blanket preferences and rebuilds native limits before returning capabilities.
AqBanking's job factory selects the supported CAMT, SWIFT or credit-card job;
no application-side statement parser or bank-specific format selector is introduced.

The worker exports rebuilt account capabilities before dispatch, including on a
local unsupported-job result. Scoped account/group refresh reloads these native
capabilities before choosing jobs, without expanding the requested account scope.
Unsupported operations are explicitly reported as limitations, preserve existing
history and do not turn into empty reports. Bank/transport/setup failures remain
errors; only a marked local capability rejection is classified as unsupported.
Refresh counts include only accounts for which data was actually obtained.

Group and overall balance completeness exclude securities accounts (types 30–39)
when no balance is stored for them, matching their exclusion from account refresh.
They remain visible and included in the account-list count. Existing stored balances
are retained in totals; genuinely missing balances on other accounts remain visible
as a partial balance. Depot holdings and valuations are not retrieved.

Errors stay visible until dismissed or the vault is locked. The details window
shows the complete selectable message and offers an explicit copy button.
Repeated socket-wait progress ("Waiting for data to become available") is not
forwarded, so it does not overwrite the current banking operation. Cancellation
and meaningful progress messages remain active.
Account synchronization publishes the account's display name and bank access,
followed by either balance retrieval or transaction retrieval. Account discovery
has its own preparation status. Its client uses a bridge clone that suppresses
native progress text, keeping these labels stable throughout each operation;
PIN/TAN/card prompts, cancellation and error reporting remain unchanged.

The worker exports only booked and disposable balances, matching the Rust
adapter's existing balance contract. AqBanking may also return empty CAMT balance
records with an unknown type. These unused records do not block the transaction
batch; missing amounts in supported balances or transactions still fail the batch.

## Reproducible inputs and local patches

`native/dependencies.json` pins version, source URL and SHA-256 for AqBanking 6.9.2,
Gwenhywfar 5.14.3 and Libchipcard 5.1.6. Source packages are verified before extraction.
The exact build environment's transitive libraries are inventoried during packaging;
those OS package versions are not pinned by this manifest.
The inventory retains original library paths while relocating dependencies, so
Linux package ownership is looked up against installed files rather than staging copies.

`scripts/build-native.py` applies explicit compatibility/security changes:

- Read camt.052.001.08 entry amounts and currencies at `Ntry`, independently of
  optional `NtryDtls/TxDtls`. The 6.9.2 XML profile otherwise looks for the amount
  in the details and can return statements without a value. Debit/credit signs
  follow the entry's `CdtDbtInd`; detail or foreign-currency amounts do not replace
  the booked amount. Parsing stays inside AqBanking. Missing/unrepresentable
  native amounts now return a job error instead of terminating the worker. The
  parent rejects that entire result batch and retains its previous transactions.
- Try subsequent UPD-authorized statement job factories if an earlier one cannot
  be created from BPD. A stale HKKAZ entry must not hide a working HKKKU/DKKKU
  credit-card job. These are local capability checks, with no bank request retry.
- Read the current HITANS `needTanMediumId` requirement in AqBanking's existing
  TAN-method loader, alongside its legacy `tanMediumIdNeeded` J/N field. Version
  6.9.2 otherwise reports no required device name for newer TAN parameters.

- Disable automatic import of another application's legacy `~/.aqbanking` data
  when an explicit directory is supplied; an exported marker prevents linking
  accidentally against an unpatched AqBanking.
- Disable creation of raw FinTS message logs in the native dialog constructor.
- Require explicit presentation of AqBanking VoP results even for all matches or
  a missing result; the adapter rejects unsupported dialogs.
- Route native chipTAN USB challenges through the optical challenge callback and
  the bounded Libchipcard adapter, avoiding the legacy upstream USB TAN parser.
- Filter Libchipcard reader selection by the exact user-selected reader name.
- Correct a C++ string-literal spacing issue for current Apple compilers.
- Release temporary SWIFT descriptors and global-data directory lists, and avoid
  allocating an account-spec list before the getter replaces it. Linux's existing
  LeakSanitizer tests exercise all three paths; leak detection stays enabled.

Review these patches on every native version update. Preserve native COPYING and
AUTHORS files. Exact original archives accompany `package-source.sh` output and
can be consumed from `native/vendor/`. Matching source and patches for bundled
transitive GPL/LGPL libraries still need collecting before a public binary release.

## Building

Use a build cache path without whitespace; the repository may contain spaces.
Default: `~/.cache/bankingcraft/native`. Python needs tar extraction filters
(Python 3.12+ recommended). Build the native runtime before running bank operations:

```sh
python3 scripts/build-native.py
python3 scripts/test-native.py
cargo build --release --locked -p bankingcraft
python3 scripts/package-native.py target/release
```

Packaging requires a fresh destination `native/` directory. Do not overwrite a
running installation. The package includes worker, plugins, data, shared-library
dependencies and licenses. macOS loader paths and Linux RPATH are made relative.
macOS minimum-version metadata is derived from the bundled Mach-O images; the
existing ad-hoc signing is retained, without adding distribution signing.

macOS prerequisites (plus Xcode Command Line Tools):

```sh
brew install pkg-config gettext gnutls libgcrypt gmp libxml2 libxmlsec1
```

Ubuntu/Debian native prerequisites (in addition to the GUI dependencies in README):

```sh
sudo apt-get install build-essential pkg-config gettext libgnutls28-dev \
  libgcrypt20-dev libgmp-dev libxml2-dev libxmlsec1-dev libpcsclite-dev pcscd zlib1g-dev patchelf
sudo systemctl start pcscd.socket
```

Windows: build the Rust GUI with MSVC. Build/package the worker in an MSYS2 UCRT64
shell, using its native MinGW Python and compiler:

```sh
pacman -S --needed make mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-python \
  mingw-w64-ucrt-x86_64-pkgconf mingw-w64-ucrt-x86_64-gettext-tools \
  mingw-w64-ucrt-x86_64-gnutls mingw-w64-ucrt-x86_64-libgcrypt mingw-w64-ucrt-x86_64-gmp \
  mingw-w64-ucrt-x86_64-libxml2 mingw-w64-ucrt-x86_64-xmlsec
python scripts/build-native.py
python scripts/test-native.py
# After building the Rust GUI with MSVC:
python scripts/package-native.py target/release
```

The CI composite action `.github/actions/native-runtime/action.yml` encodes these
steps. It runs offline runtime checks, bundles a test runtime and scans it. Native
ASan/UBSan adapter tests run on Unix. Windows runtime tests do not use sanitizers.

The worker intentionally uses backend APIs from the pinned AqBanking source tree.
Its bundled library exports those symbols on ELF and Windows, matching the macOS
build. The build disables hidden symbol visibility only for AqBanking and enables
MinGW DLL exports there. It does not change the installed system libraries; the
worker still requires the patched `AB_BankingCraft_NoLegacyImport` marker. Windows
uses UCRT64 consistently for the worker, libraries, packaging and audit inventory.
Gwenhywfar uses its Win32 path discovery there; its Unix `/proc` binreloc code is
enabled only on Unix.
On Linux the built worker and sanitizer test binary locate their bundled libraries
via `$ORIGIN/../lib`. Token-plugin initialization tests require a running PC/SC
service, but do not open a card or request a PIN.
The ephemeral GitHub Linux runner grants its `runner` user permission to establish
a PC/SC context because CI has no active desktop session. That rule grants no
permission to access cards, and is not installed by application packages.

## Verification and inventory

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/test-native.py
python3 scripts/test-native-adapter.py
python3 scripts/package-native.py /tmp/bankingcraft-runtime-review
python3 scripts/audit-native.py /tmp/bankingcraft-runtime-review/native
python3 scripts/update-licenses.py --check
```

The native tests use a synthetic home/configuration and `bank.invalid`; they do
not submit bank requests. They verify state round trips, isolation from legacy
settings, no configuration file writes, malformed IPC, disabled raw message logs,
TLS error rejection and explicit accept/reject/cancel on the actual AqBanking VoP
dialog. Injected administrative callbacks verify the setup order, failed setup and
later explicit retry, optional anonymous BPD, and persisted initialization without
sockets. Rust refresh tests check that access/transport/PIN failures stop further
account requests while preserving partial data. Sanitizers instrument our C adapter,
not every upstream library. These checks cannot prove a particular bank accepts a
live login; the screenshot's final 9999 response alone does not identify its original
rejection.

Device-name tests load synthetic legacy/current parameters through the real AqBanking
user reader, check prompt routing, Unicode length, exact persistence, cancellation,
and repair before any login. CAMT tests rebuild real account specs from synthetic
CAMT-only, MT940-only and mixed CAMT/credit-card BPD/UPD, including unsupported
accounts and UID-specific permissions. SEPA tests exercise AqBanking's real HISPA
importer, matching leading-zero account numbers and subaccounts, and verify the
resulting job's IBAN/BIC/account arguments plus migration/retry marker behavior.
Process-level regressions restore the same synthetic snapshot in separate workers,
check that credit-card permissions survive, and ensure unsupported requests return
fresh capabilities and specific details without bank I/O. Snapshot tests preserve
repeated UPD/TAN groups and verify the one-time parameter-refresh migration.
Synthetic card-token tests select a matching non-first context, reject missing or
ambiguous bank/user matches, stop on cancellation, and verify the actual adduser
arguments without accessing a physical card or sending bank requests.
Read-session tests count synthetic token opens/closes across balance and transaction
queues, reject payment dispatch and mismatched account IDs, and require a fresh
open after teardown. Process tests reject identity/snapshot changes, support EOF
cleanup, and keep local unsupported jobs within the same session. Rust tests check
worker reuse, state preservation on errors and invalidation after vault locking.

The audit script pins Syft/Grype archives by SHA-256, generates a CycloneDX SBOM
and scans it locally. Mac Homebrew, Linux package and Windows MSYS2 identities
supplement binary detection. It fails on known vulnerabilities at low severity or
higher, without a suppression list. CPE matching for generic native components
is heuristic; zero findings is not proof of absence of vulnerabilities.

Follow-up validation requires other bank configurations, TAN modes, VoP, DDV/RDH and
chipTAN readers, payment rejection/uncertainty scenarios, and all three operating
systems. Historical successes of the former backend do not cover these paths.
