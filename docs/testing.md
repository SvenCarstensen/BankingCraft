# Testing

## AqBanking migration and removal — November 15, 2026

The current runtime uses AqBanking and Libchipcard. The old `fints` and `cardreader`
implementations, protocol simulator tests, custom cryptography and bank-response
parsers have been removed. Shared models/local validation live in
`crates/banking-models`; scoped-refresh tests use a backend double. Export tests
inspect XML directly. Older test counts and bank/hardware successes below are
historical and do not validate the native integration.

See [native banking](native-banking.md) for build/test commands and
[security hardening](security-hardening.md) for internal evidence and open external
review. Automated checks are offline/synthetic. Separate authorized live checks on
macOS completed Postbank PIN/TAN account, balance and transaction retrieval, including
credit-card transactions. The user also confirmed the account/operation progress
display. No real payment or physical card workflow was exercised.

The final local workspace run passed 151 tests, with 8 ignored by the default runner;
Clippy with warnings as errors and the macOS release build passed. Native offline
and ASan/UBSan regressions cover repeated snapshot groups, setup/TAN-medium repair,
account capabilities, credit-card job selection, CAMT entry amounts and currencies,
unused empty balances and malformed amounts. A UI-backed synthetic refresh test
checks the account/balance/transaction status sequence for full and scoped refreshes,
including native progress suppression and shared cancellation.


Run from the repository root:

```sh
./scripts/test-business.sh
```

The script first uses Python 3 to verify the license bundle against the lockfile,
package manifests and recorded hashes. It then runs tests for every Rust package
and Clippy with warnings treated as errors. It does not change a production vault,
contact a bank or require a card reader. Tests use fictional accounts, temporary
encrypted vaults and simulated FinTS/PayPal transports. Unexpected network calls
fail the relevant flow test.

## Historical verification — November 14, 2026

- Bank-directory regression tests retain newer Sparda, Marcard, Warburg, Renault
  and IKB URLs while checking the verified Commerzbank/NATIONAL-BANK host updates.
- PayPal tests cover encoded API credentials, fixed live/sandbox destinations,
  read-only methods, signed net amounts and fees, separate currencies, 100-result
  limits, warning 11002, overlapping windows and conflicting duplicates.
- Invalid responses, unsupported financial records, incomplete results, cancellation
  and failed requests cannot produce a successful partial snapshot. Only complete
  snapshots are committed through the encrypted vault's atomic edit path.
- Stable PayPal IDs replace pending entries without losing manual categories.
  Scoped refreshes preserve other wallets and older history. Available PayPal funds
  are not added to pending amounts or presented/exported as a booked balance.
- Tests verify encrypted credentials on disk, password changes and rollback when
  credentials are corrupt; form masking and clearing on cancel/provider/vault changes;
  exclusion from payment accounts; and currency separation in summaries and EUR reports.
- PayPal setup screenshots were rendered in German, English, French and Spanish at
  1100 and 1360 pixels. German mixed bank/PayPal balance views were also inspected.
  These use synthetic data only; no real PayPal credentials or live requests are used.
- The full workspace suite passed **248 tests**, with six explicit screenshot exporters
  ignored by default. Clippy and the normal macOS release build passed. Historical
  counts below belong to their dated implementation stages. GitHub CI checks the
  corresponding revision on all three supported operating systems.

For reproducible PayPal visual checks:

```sh
cargo test -p bankingcraft --locked render_paypal_setup -- --ignored
cargo test -p bankingcraft --locked render_paypal_balances -- --ignored
```

These write `bankingcraft-paypal-*.png` into the operating system's temporary directory.
PayPal remains experimental and unverified with a real account. Only Postbank PIN/TAN
and VR Bank Nord RDH-7 retrieval have been confirmed in the tested real-bank setups.

## Automation and synchronization — November 14, 2026

The GitHub workflows in `.github/workflows/` test macOS 15, Windows 2025 and Ubuntu
24.04 with Rust 1.99.1: license hashes, workspace tests, Clippy and normal release
builds without `local-fints-test`. They use no real banking data or test product ID.
RustSec checks also run weekly. Known vulnerabilities, unsoundness and yanked
packages fail the audit; unmaintained-package warnings remain visible.

**Synchronize transactions** first sends saved orders for the selection. Global
synchronization processes all outgoing orders before the first balance/transaction
request, including across bank connections. Single-account and group synchronization
remain scoped to their accounts. Errors or cancellation stop the operation before
retrieval; uncertain orders are not sent again. Regression tests verify ordering,
fresh bank state after submission and durable protection against duplicate submission
using synthetic vaults and simulated sends. PayPal itself has no payment operations.

## Font dependencies and license bundle — November 13, 2026

- System-font discovery moved to Fontique; egui continues to render with skrifa 0.44.
  Preferred families, weights and TTC indices are preserved.
- `ttf-parser`, `owned_ttf_parser`, `ab_glyph` and `fontdb` were removed from the entire
  lockfile. Wayland title bars use Crossfont/FreeType; the local Crossfont re-export
  only aligns the Fontconfig binding with Fontique.
- At that stage, 227 automated tests passed, three manual screenshot exports were
  ignored, and Clippy had no warnings. Sixteen German/French transfer, order,
  transaction and standing-order screenshots at 1100/1360 pixels were compared
  separately: before and after images were byte-identical on that macOS system.
- The macOS release build passed. In a Linux x86_64 container with Rust 1.99.1,
  `cargo check -p bankingcraft --locked --all-targets` passed, including Wayland
  decorations and UI-test dependencies. This was not an interactive Linux test.
- Cargo Audit reported no known vulnerabilities and three remaining unmaintained
  zbus-related packages: `zbus_names`, `zvariant` and `zvariant_derive`.

## Covered workflows

First-vault setup offers a checked-by-default checkbox for one local demo account
with sample transactions. `first_vault_offers_demo_by_default_and_saves_the_checkbox_choice`
checks both choices through the actual creation dialog, encrypted reopening, password
validation, filename changes and exclusion from subsequent vaults. The existing full
demo-bank option still adds both demo accounts and standing orders.

### Balances including pending transactions — November 14, 2026

For bank accounts, the primary display adds only pending transactions from the same
account and currency to the bank's booked balance. **Booked balance** stays visible
separately. A bank's available amount, which may include a credit facility, is not
used as a substitute. Daily closing balances and statistics use booked transactions.
PayPal is explicitly different: its API supplies available funds only, so its booked
balance is unavailable and pending amounts are not added a second time.

`view::amount_tests` covers debits, credits, repeated pending entries, missing balances,
separate currencies, group/global totals and the pending-to-booked transition without
double counting. `balance_labels_and_amounts_are_separate_and_visible_in_all_languages`
checks labels in Overview, Accounts and Transactions at 1100/1360 pixels in all four
languages. Synthetic views can be exported for inspection:

```sh
cargo test -p bankingcraft --locked render_balance_review -- --ignored --nocapture
```

### Workflow matrix

| Workflow | Cases covered | Tests |
|---|---|---|
| Bank connection and dialog | Synchronization, system ID, opening/closing/reopening dialogs, missing connections, offline demo | `client::business_flow_tests`, `jobs::business_tests` |
| Account discovery and refresh | IBAN enrichment, subaccounts, distinct bank connections, stable IDs, balances, group assignments and ordering | `business_tests`, `client::business_flow_tests`, BPD tests |
| Bank balances | HKSAL versions 5/6/7, debit/credit, available amount, account permissions, standard/proprietary credit-card balances | `client::business_flow_tests` |
| Current-account transactions | MT940/MT942, CAMT, pagination, pending entries, empty results versus missing data, missing/repeated continuation tokens | `client::business_flow_tests`, `business_data_tests` |
| Credit-card transactions | HKKKU/DKKKU, field positions, booking/value dates, signs, references, pending status, report balances, pagination, corrupt/incomplete responses | `credit_card::tests`, `client::credit_card_regressions` |
| Local merging | Repeated/overlapping retrievals, legitimate identical bookings, pending-to-booked changes, account-scoped replacement, manual categories | `business_tests` |
| Single-account refresh | Balance/transaction requests only for the selected account, no account discovery, other accounts/connections unchanged, separate outgoing-payment phase, demo and invalid IDs | `jobs::account_refresh_tests` |
| Group refresh | Only the selected group's accounts, one dialog per bank connection, multiple banks, ungrouped accounts, empty/invalid groups, unrelated orders unchanged | `jobs::account_refresh_tests` |
| Transaction navigation | Synchronization uses the current selection and stable ID; an invalid selection never starts a global refresh; Back returns to Accounts | `transactions_ui::tests` |
| Transfers | Amount/IBAN/text validation, standard and instant transfers, no fallback to standard transfers, no automatic retry after bank error/response loss | `client::business_flow_tests`, `business_data_tests`, `regression_tests` |
| Outgoing orders | Encrypted persistence/restart, selected accounts/connections only, all payments before retrieval, updated dialog state, individual immediate send, durable pre-send marker, no duplicates after response loss/crash/write failure, cancellation and expired dates | `jobs::outgoing_order_tests`, `payments::tests` |
| Recipient address book | Explicit checkbox opt-in, same IBAN separated by source account, name/IBAN validation, independent order snapshots, selection/account changes, checkbox reset, old vaults and connection removal | `payments::tests`, `payments_ui::tests` |
| Default action | Enter in the transfer form saves without contacting a bank; recipient selection fills name/IBAN/BIC; encrypted round trip of orders/address book | `payments_ui::tests` |
| Scheduled transfers | Missing/current date rejected, future execution date in payment document, order-ID handling, unsupported bank operation | `client::business_flow_tests` |
| Create standing order | Monthly/weekly, interval, final date, month-end rules, limits, placeholder date in payment document, TAN | `client::business_flow_tests`, `business_data_tests` |
| Retrieve standing orders | HKCDB 1/2, format negotiation, pagination, execution time/final date, invalid versions/formats, incomplete pages | `client::standing_regressions`, `client::business_flow_tests` |
| Delete standing order | Preserve original bank data/order ID, TAN, unsupported operation, missing local order | `client::business_flow_tests`, `jobs::business_tests` |
| Next execution | Leap years, short months, quarterly/fortnightly schedules, inclusive final date, month-end minus one/two days | `business_tests` |
| TAN and approval | Permitted methods, TAN follow-up without payment replay, cancellation, app approval/polling, invalid challenge, SCA for reads, PIN rejection | `client::business_flow_tests` |
| TAN display and card protocol | Matrix payloads/lengths, invalid flicker codes, USB BCD TAN, card status, APDU/TLV, DDV encryption/signature counters | `business_data_tests`, `regression_tests`, `cardreader` |
| RDH | Complete simulated RDH-7 dialog, bank keys, hash matching, signing/encryption/decryption, tampered/unsigned responses | `rdh_flow_test`, `rdh::tests`, `regression_tests` |
| Verification of Payee | Match/close match/no match/unavailable, complete bank notices, XML/native reports, payment/IBAN binding, cancellation before TAN, identical second submission, polling without replay, smart-card flow, unsupported/invalid replies | `vop::tests`, `client::business_flow_tests`, `payee_ui::tests` |
| Multiple vaults | Exclusive creation including symlinks, separate passwords/accounts, legacy default, last selection, corrupt selection file, cancel/wrong password, busy-operation lock, form/PIN/API-secret clearing, save failure during switch | `vaults::tests` |
| Vault storage | Encrypted save/open, fresh nonces, permissions, wrong password, tampering/truncation, failed file replacement | `business_tests` |
| Passwords and saved PINs | Password changes with secret re-encryption, rollback, corrupt secrets, vault binding, explicit opt-in, removal after bank rejection | `store::tests`, `business_tests`, `jobs::business_tests`, `paypal::tests` |
| Groups and account ordering | Create/rename/delete/assign, duplicate/invalid names, repeated assignment, forward/backward movement, invalid targets, persistence | `groups::tests`, `business_tests` |
| Group balances | Booked/pending amounts, separate currencies, partial totals, missing versus zero balances, available-only PayPal funds | `view::amount_tests`, `paypal::tests` |
| Transaction view | Combined account/date/category/multiword search, case folding, unknown dates, pending first, amount/date formatting, per-currency totals | `view::amount_tests`, `business_tests`, `paypal::tests` |
| Categories and analytics | Manual before learned before default rules, salary only for credits, month boundaries, exclusion of pending/undated/internal transfers, monthly series, EUR-only analytics | `business_tests`, `view::amount_tests`, `paypal::tests` |
| Bank X import | Repeatability, normalization, preserved configuration/groups, format/field/account validation, size limit, encrypted backup, no partial import | `bankx_import::tests` |
| CSV/CAMT export | Selection/order, invalid indices, special characters including CR, unknown dates, pending/debit/credit, references, CAMT re-import; PayPal available balance and non-IBAN identifiers | `business_tests`, `paypal::tests` |
| Remove bank connection | Remove associated accounts, transactions, standing orders and group assignments; preserve other connections | `business_tests` |
| Bank search and profiles | Common banks/search terms, verified host updates, retained newer URLs and whitespace handling | `banks::tests` |
| PayPal retrieval | Signature credentials, read-only endpoints, pagination, errors, net amounts/fees, currency scopes, stable IDs, credential encryption and available-balance semantics | `paypal::client::tests`, `paypal::tests` |
| Security | Incomplete TAN approval, debtor account, HTTPS targets, parser/XML/image limits, bank keys, DDV envelope/padding, RDH-10 vector, CSV formulas, private files/symlinks | `security_tests`, `client::business_flow_tests`, `rdh::tests`, `bridge::image_security_tests`, `private_file::tests`, `business_tests` |
| Window restoration | Position/logical size, repeated saves, invalid/oversized settings, negative coordinates, Retina scaling, disconnected monitor | `window_state::tests` |
| Appearance | First-run system theme, missing/invalid/oversized settings, repeated changes and persistence of all three modes, write failures, return to system theme | `appearance::tests` |
| RDH card PIN | ASCII PC/SC reference vector without padding, exact software-PIN length, FPIN2 length nibble/padding, access-PIN selection only, unknown/missing metadata, no second attempt after failure/cancel/transport loss, success cache, retry counter and reader responses | `cardreader::rdh::tests` |

## Localization

Language tests cover German, English, French and Spanish, regional system languages,
English fallback, missing/invalid preference files, persistence/restoration and write
failures. Every catalog translation must preserve format placeholders. Formatting tests
cover Unicode, named/positional values and language-independent cancellation handling.

A UI test loads all four embedded translations and refreshes the view models while
preserving category IDs, filters, account/group names, bank text and form drafts.
Tests use synthetic data and never modify production preferences.

## Account names

The Accounts page supports independent group disclosure and drag-and-drop ordering.
`grouped_accounts_drag_between_rows_collapsed_empty_and_ungrouped_targets_atomically`
exercises pointer gestures at 1100/1360 pixels, encrypted save/reopen, cross-group
moves, cancellation and save failure. It also checks disclosure state after refresh,
navigation, rename and vault changes, and that account actions remain clickable.
`group_drops_validate_before_editing_and_preserve_unrelated_accounts` checks invalid
destinations and the relative order of unrelated accounts. The visual exporter below
also renders a collapsed group.

`accounts_ui::tests` covers local aliases, unchanged bank-provided account data,
refresh and old vault formats, encrypted persistence/reopening, cancel/reset and
rollback after save failure. View models preserve account selection. Drag-and-drop
is tested with actually wrapped rows, after reordering and resizing, in both directions.

For an offline visual check of Overview, Accounts and the rename dialog:

```sh
cargo test -p bankingcraft --locked render_account_names -- --ignored
```

The exporter uses synthetic accounts with long names and writes German/French views
at 1100/1360 pixels to `bankingcraft-egui-account-name-review` in the temporary directory.
Inspection on November 13 confirmed visible names, appropriate row heights and readable
actions/dialogs. The export also includes DDV/RDH setup forms: reader, card-record and
read-card controls are vertically centered without overlap, with notes on their own row.

## English demo screenshots

`view::overview::tests` checks independent group filters on the four overview tiles:
groups spanning bank connections, ungrouped accounts, empty groups, filtering before
row limits, monthly/category totals excluding pending entries and internal transfers,
group rename/reorder/delete, account assignment, and language/vault changes. Global
totals, other tiles, the complete standing-order list and vault data remain unchanged.
The visual exporter covers the overview at 1100/1360 pixels. `render_payment_pages`
also covers the standing-order form's vertically centered schedule/date row in
German and French.

The five README views use the real egui pages and fictional English demo data:

```sh
cargo test -p bankingcraft --locked export_english_showcase -- --ignored
```

The exporter writes Overview, Accounts, Transactions, Transfers and Reports PNGs to
`docs/screenshots`. It never opens an existing vault, reads a local product ID or
contacts a bank. Optionally, `BANKINGCRAFT_DEMO_VAULT` (a new path outside the repository)
and `BANKINGCRAFT_DEMO_PASSWORD` create a separate encrypted demo vault. Existing files
are never overwritten. Without those variables, only images are generated; demo dates
are relative to the day of execution.

## egui/eframe 0.36

The UI uses `egui_kittest` through the actual `eframe::App` entry point. Interaction
tests exercise Enter in the transfer form, account navigation, group filters and
drag-and-drop. Layout tests cover four languages at 1100/1360 pixels: standing-order
fields stay aligned and long account names do not widen the transaction filter.
TAN dialogs keep Cancel and Approve reachable in small windows with long bank notices
and maximum flicker size. Selecting a different vault clears password drafts.
Dispatcher tests cover UI-thread boundaries, event order, reentrant callbacks and timers.

English README images use the same UI functions through the offscreen renderer and
never read production preferences. The native build uses OpenGL; text antialiasing
can differ from the former renderer.

After the November 13 migration, 213 regular workspace tests and Clippy passed.
The three separate screenshot exporters also passed. English demo pages were inspected
in both themes; long German/French names and payment forms were checked at two widths.
The release build was installed as an ad-hoc-signed macOS app and started, preserving
window size, German language and the existing vault selection. The unlock screen was
checked with the native OpenGL renderer. No production retrieval or payment was triggered.

## Limitations and visual inspection

For reproducible offline inspection of transfer forms and the outgoing-order list:

```sh
cargo test -p bankingcraft --locked render_payment_pages -- --ignored
```

This also covers Transactions with Back to Accounts and the VoP result dialog with
long notices in a narrow window. Only synthetic data is rendered to PNG files in
`bankingcraft-egui-payment-review` in the temporary directory. German/French layouts
are checked at 1100/1360 pixels. This explicit exporter is ignored by the regular
suite; its separate run and inspection passed on November 13. The checkbox, both
transfer actions, account-specific address book and all order states were readable
in the dark theme without clipped labels.

Tests cover business behavior and protocol flows, not every line of code. Simulator
success does not establish bank certification or hardware compatibility. Tests never
send real payments. Historical no-reader checks below preceded the later confirmed
VR Bank Nord RDH-7 retrieval.

### Historical native checks before the egui migration

These checks do not replace inspection of the current renderer. The installed macOS
app was tested with vault unlocking, pending transactions, category chips/selection
and compact rows. Status/category chip centers and their labels were vertically aligned.
Postbank credit-card communication was separately tested with read-only retrieval.

On November 13, appearance was checked in the native app: no preference file selected
the dark system theme; switching between light/dark worked; light mode survived a full
quit/restart despite a dark system setting. System mode was restored afterward.
Overview, Transactions, Transfers and Settings were inspected in dark mode. The macOS
system theme itself was not changed.

Localization was also checked in the rebuilt app: without a language preference, it
used the German system language. French, Spanish and English took effect immediately.
Settings and French transfer forms were inspected for clipping. Spanish Transactions
showed translated categories, date headings and pending status while preserving bank
text. English survived a full restart on the unlock screen and overview. The app was
then returned to system language (German). These UI checks triggered no bank request
or payment.
