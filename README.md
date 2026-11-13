# BankingCraft – Desktop banking for Windows, Linux and macOS

BankingCraft is a Rust desktop banking application with a Slint interface: a navigation sidebar,
an account dashboard, transaction lists with daily balances, payment forms and financial reports.

The application was previously called Kontor. Existing vaults are still found in their original
location; the file format, `vault.kontor` filename and key derivation remain compatible.

## Privacy and data transmission

BankingCraft does not collect or transmit telemetry or usage analytics. Personal and financial
data stays on your device and is not sent to the developers or other third parties. The only
exception is the data exchanged with your configured bank, including its banking service provider,
to authenticate, retrieve account information and submit banking orders. Banking data is stored
locally in the encrypted vault. Exports are saved locally; sharing an exported file is your choice.

## Features

| Area | Implementation |
|---|---|
| Current accounts | UPD account discovery, IBAN/BIC (HKSPA), booked and available balances (HKSAL v5–7), camt.052 transactions (HKCAZ) with MT940/MT942 fallback (HKKAZ v5–7), pending transactions, pagination and duplicate detection |
| Credit cards | HKKKU v1 / DKKKU v2 transactions; HKKKS v1 / DKKKS v3 balances or balances from transaction reports; account permissions, pagination, debit/credit and pending entries |
| PIN/TAN | FinTS 3.0, PSD2 SCA (HKTAN v6/v7, processes 4/2/S), system synchronization, TAN method/media selection and decoupled app approval |
| TAN display | Optical chipTAN with adjustable animated flicker code, photoTAN/QR-TAN images from HHDuc, SMS/text |
| chipTAN USB | HHDuc sent through `FEATURE_MCT_READER_DIRECT` (SCardControl) or `FF 91 06 00` APDU; TAN read back from the reader |
| HBCI smart cards (DDV) | DDV-0/DDV-1; bank details and signature counters read from the card; RIPEMD-160 and card-generated DES retail MAC; two-key 3DES CBC encryption; HBCI over TCP port 3000 |
| RDH smart cards | ZKA/SECCOS cards such as VR-NetWorld-Card, RDH-7/9/10 profiles, card notepad access, HKISA bank keys checked against the card hash, card-generated RSA-PSS signatures and signature counters, two-key 3DES and RSA key transport (PKCS#1 v1.5 for RDH-7/9, mode 2 for RDH-10), bank signature verification |
| Secure PIN entry | `FEATURE_VERIFY_PIN_DIRECT` for class 2/3 readers; the PIN stays on the reader. Software PIN entry is available only for readers without a PIN pad |
| Payments | SEPA transfers (HKCCS), instant transfers (HKIPZ), scheduled transfers (HKCSE), standing order creation/retrieval/deletion (HKCDE/HKCDB/HKCDL), pain.001.001.03/.001.003.03/.001.001.09 |
| Reports | Automatic categorization with learned rules, monthly and twelve-month reports |
| Export | CSV with translated headings and categories, semicolon delimiters and German number/date formatting; camt.052.001.08; files saved to Downloads |
| Bank directory | 2,787 institutions with PIN/TAN URLs or HBCI hosts; search by name, city, bank code or BIC, tolerant of German umlaut spelling; embedded in `app/data/banks.tsv` |
| Privacy | Local AES-256-GCM vault, key derived from the vault password using Argon2id; online banking PINs are saved only when explicitly requested and are additionally encrypted; card PINs are never saved |
| Appearance | Light and dark themes, applied immediately; the first launch follows the system theme |
| Languages | German, English, French and Spanish; the first launch uses the system language, with English as the fallback; the language can be changed in Settings |

## REINER SCT cyberJack (secoder)

Readers with “cyberJack” or “REINER SCT” in their PC/SC name are detected and preferred.
**Card reader → Check reader & card** shows the reported PC/SC Part 10 capabilities
(secure PIN entry, MCT/chipTAN USB, display), the inserted HBCI card and its bank connections.

The proprietary Secoder display of payment details during HBCI signing is not implemented.
For chipTAN USB, the reader displays payment details using the standard HHDuc mechanism.

Drivers: REINER SCT drivers on macOS and Windows; `pcscd` and `ifd-cyberjack` on Linux.

## Build

Requirements: stable Rust (tested with 1.99.1), plus:

- **Linux:** `libpcsclite-dev`, `pcscd`, `libfontconfig1-dev`, `libxkbcommon-dev`
- **macOS:** Xcode Command Line Tools; PC/SC uses `PCSC.framework`
- **Windows:** no additional PC/SC package; WinSCard is part of Windows

```sh
cargo build --release
./target/release/bankingcraft          # Windows: target\release\bankingcraft.exe
```

**macOS application:** double-click `macos/build-macos.command`. It builds `BankingCraft.app`
next to the project directory, includes the icon, applies an ad-hoc signature and launches the app.

For environments without GPU/OpenGL support, use
`SLINT_BACKEND=winit-software ./target/release/bankingcraft`.

Set `RUST_LOG=debug` for protocol diagnostics. Message contents and PINs/TANs are not logged.
Error reports contain technical segment summaries, without free-form bank messages.

## Getting started

1. Set a vault password.
2. Under **Settings → FinTS product registration**, enter a product ID registered with
   Deutsche Kreditwirtschaft. See [the official registration page](https://www.fints.org/de/hersteller/produktregistrierung).
3. Open **Bank connections → New bank connection** and find the bank by name, city,
   bank code or BIC. The bank code and FinTS URL or HBCI host are filled in automatically.
   Enter the PIN/TAN user ID, or select a smart card reader and choose **Read data from card**.
4. Choose **Retrieve transactions** on the overview page.

Without a bank connection, **Load demo data** creates a local demo connection. Demo transfers
show a sample flicker code and are simulated locally; they do not contact a bank.

Window size and position are saved when the window closes or the application quits and restored
on the next launch, including the locked screen. `window-state.json` next to the vault contains
only window preferences. If a previous monitor is unavailable, the window is moved to an accessible
screen. On Wayland, the compositor may restrict restoring absolute window positions.

Under **Settings → Appearance**, choose **System setting**, **Light** or **Dark**.
Without a saved choice, the application follows the system, including later theme changes.
A manual choice takes effect immediately and is stored in `appearance.json` next to the vault.
It also applies to the unlock screen; the file contains no banking or credential data.

Under **Settings → Language**, choose **System setting**, **Deutsch**, **English**, **Français**
or **Español**. On the first launch, the application uses the primary system language; regional
variants such as `fr-CA` and `es-MX` are recognized. Unsupported or unavailable languages fall back
to English. The selection takes effect immediately and is saved in `language.json` next to the vault,
so it also applies before unlocking on the next launch. System mode resolves the system language
at startup. User-created group names, account names and bank-supplied transaction descriptions
remain unchanged. Category IDs and payment data remain stable across language changes.
Numeric dates and financial amounts retain their existing input/output conventions.

## Account groups

Create and rename groups under **Accounts**. Assign an account using its group selector;
**Ungrouped** removes the assignment. Removing a group preserves accounts, bank connections
and transactions. Groups are encrypted along with the vault.

The overview shows each group’s known booked balances, separately by currency. Missing balances
are marked as missing rather than treated as zero. Drag accounts within the overview account list
to reorder them; an insertion line shows the destination. The order survives application restarts
and account refreshes, and sorting preserves group assignments.

Standing order retrieval negotiates HKCDB version 1 or 2 and the bank’s supported pain.001 formats.
Version 2 also supplies execution times. Failed retrievals preserve existing standing orders and
are reported as failures or partial successes.

## Project layout

```
crates/fints         FinTS 3.0 syntax, messages, PIN/TAN/DDV/RDH security, BPD/UPD,
                     banking operations, MT940, camt.052, pain.001 and flicker codes
crates/cardreader    PC/SC, DDV/RDH smart cards, chipTAN USB, PIN pads and diagnostics
crates/i18n          Shared translation catalogue, locale resolution and runtime messages
crates/i18n-macros   Compile-time checked translated format strings
app                  Slint UI, encrypted vault, categorization and background jobs
```

`crates/i18n/catalogue.json` is the translation source of truth. Each German source string maps
to English, French and Spanish translations. Slint translations are generated and bundled at build
time; Rust format strings are checked by the compiler in every language. Add new user-facing text
to this catalogue and use `@tr`, `i18n::tr` or `i18n::tr_format!`. Do not translate protocol values,
persisted identifiers or user/bank-supplied text. Keep this README and commit messages in English.

## Bank directory

`app/data/banks.tsv` originates from the `blz.properties` file in
[hbci4java](https://github.com/hbci4j/hbci4java) (LGPL-2.1, October 2026 snapshot).
Old endpoints were updated: `hbci-pintan.gad.de` to `fints1.atruvia.de`, `hbci11.fiducia.de` to
`fints2.atruvia.de`, and former Dresdner Bank addresses to Commerzbank. The bank’s own information
remains authoritative; the endpoint can be edited in the connection form.

## Compatibility with AqBanking/GnuCash

Message structure and dialog flow follow AqBanking, the FinTS library used by GnuCash
(behavior reimplemented; no AqBanking source code copied):

- System IDs obtained with a single-step dialog: method 999, security profile `PIN:1`, no HKTAN.
  Two-step dialogs use `PIN:2` and HKTAN processes 4 → 2/S.
- The TAN medium, such as a BestSign device, is placed in HKTAN field 11 in every process step.
- HKVVB protocol dialog language remains 1, independently of the UI language. SEPA orders use
  complete KTI data; HKCDB/HKCDL field ordering follows AqBanking.
- Decoupled approval uses HKTAN process S, detected through HITANS or return code 3955.

### RDH smart cards compared with AqBanking/GnuCash

**Review dated November 12, 2026: equivalence has not been established.** GnuCash uses
AqBanking/AqHBCI and libchipcard for ZKA cards. BankingCraft reimplements part of this functionality in Rust.

| Area | BankingCraft after review |
|---|---|
| RDH-7/9/10 cryptography | SHA-256/SHA-256, RSA-PSS, two-key 3DES and profile-specific RSA key transport; tested locally with RSA-2048 |
| Response validation | Personalized RDH responses must be fully encrypted and validly signed. Missing keys/signatures, inconsistent control references and unprotected additional segments are rejected |
| Card access | ZKA/SECCOS notepad, private keys, fresh signature counter and PIN pad; corrected EF_KEYD descriptors and extended APDU lengths |
| Trust | Bank keys checked against the card hash or explicitly verified by the user; no automatic acceptance of unverified keys |
| Scope | Previously initialized RDH smart cards; no software key files, initial INI setup, key changes/revocation or certificate workflows |
| Real-world validation | No confirmed successful end-to-end retrieval with a real bank and smart card yet |

Findings, sources, fixes and test limits: [RDH code review](docs/rdh-review.md).

### Import from Bank X

BankingCraft can import prepared RDH account settings from `bankx-import.json` next to the vault
on the next unlock. This is a custom, versioned interchange format, not a parser for Bank X account
folders or migration archives. The file contains bank connections and account details only: no PINs,
private keys, bank keys, balances or transactions. Create it with private file permissions.

The import validates IBAN, bank code and account number, adds missing accounts without overwriting
existing connections, backs up the encrypted vault as `vault.before-bankx-import` and removes the
import file only after saving successfully. Cards are matched by bank code and user ID; the reader
is detected automatically when none is selected. Account permissions, types and current balances
are updated from the bank during retrieval. The format is documented in `app/src/bankx_import.rs`.

## Verification

Run the offline regression suite with `./scripts/test-business.sh`. Test cases and limitations are
listed in [docs/testing.md](docs/testing.md). The [security review](docs/security-review.md) covers
fixes, the dependency scan and remaining limitations.

- Flicker code generation matches all six reference vectors from the hbci4java test suite.
- Tests cover message structure, escaping, binary data, MT940/SEPA fields, pain.001 round trips and IBAN validation.
- DDV and chipTAN USB APDU sequences follow the hbci4java `smartcardio` implementation.
- A complete RDH-7 dialog is tested against a simulated bank using real RSA-2048 keys: key retrieval,
  hash validation, synchronization, signing, encryption and bank signature verification. Card layouts
  such as the notepad, EF_KEYD and PIN formats use test fixtures.
- **Postbank PIN/TAN:** real account, balance and transaction retrieval succeeded on November 12, 2026
  after the camt fix; credit card balances and transactions were also retrieved successfully.
- **RDH:** successful operation against a real bank/card has not been confirmed. The latest checks
  cover PIN format handling and prevention of automatic retries after a failed attempt.
- No real payment orders have been submitted for testing.
- Language tests cover first-start detection, English fallback, persistence, all four bundled UI
  languages, translated format placeholders, stable categories and unchanged account/group/form data.
- Direct debit reversal is not implemented. Standing orders are changed by deleting and recreating them.
