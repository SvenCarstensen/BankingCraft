# Security review — November 12, 2026

This is a historical review of the implementation before the egui migration. For
current checks and the November 14 PayPal work, see [Testing](testing.md) and the
[bank compatibility review](bank-compatibility.md). The current UI uses egui/eframe;
the Slint dependencies mentioned below describe the old build. Postbank PIN/TAN
and VR Bank Nord RDH-7 retrieval have since been confirmed in the tested setups.
Other bank connections, PayPal and real payment submission remain unverified live.

The review covered vault and PIN storage, exports/diagnostics, transport, the FinTS
parser, TAN approval, payment orders, RDH/DDV and resolved Rust dependencies.
Synthetic regression tests exercise the changes without real credentials or payments.

## Findings addressed

| Finding | Change and verification |
|---|---|
| Outdated RSA dependency with a known timing attack | Updated `rsa` from 0.9.10 to 0.10.0. Bank-signature verification, a complete simulated RDH-7 dialog and an independent RDH-10 RSA vector verify the migration. Production private user keys remain on the smart card. |
| Incomplete TAN approval could be treated as success | Pending approval codes, a missing order reference or a missing final success response now cause an error. Payments are not retried automatically. |
| Payment document could specify a different debtor account | The selected account's IBAN must match the payment document for transfers, scheduled transfers and standing orders. |
| Spreadsheet formulas in bank-supplied text | Potential formula prefixes, including leading whitespace/BOM, are neutralized in CSV text fields. Ordinary numeric amounts remain numeric. |
| Overly permissive file access and predictable export paths | CSV/CAMT files are created exclusively with randomized names and Unix permissions 0600. Diagnostic files are replaced atomically with private permissions. Existing files/symlinks are not opened as write targets. |
| Bank text in persistent diagnostics | Error reports contain only technical segment summaries. Runtime logs restrict bank responses to validated numeric codes. Business error messages remain visible in the UI. |
| Unnecessarily long secret lifetime | PIN caches, message buffers and transport encodings use `Zeroizing`; rejected PINs are also removed from the memory cache. Password fields are cleared after submission. |
| Ambiguous HTTPS destinations | URLs containing credentials, fragments, backslashes or control characters are rejected before transmitting a PIN. Certificate verification remains enabled and redirects remain disabled. |
| Excessive memory use from input data | Limits cover message sizes, segment/field counts, XML nodes, TAN image dimensions/memory, bank keys and vault files. XML DTDs remain forbidden. |
| Insufficient validation of encrypted card responses | DDV requires a matching encrypted envelope without additional injected plaintext segments. RDH/DDV validate block sizes and padding; bank keys are validated before storage. |

The RSA update follows [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html).
CSV handling follows [OWASP CSV Injection](https://community.owasp.org/attacks/CSV_Injection).
CSV cannot provide a universal guarantee across all spreadsheet applications and later
save/import operations; this mitigation addresses the initial interpretation of exported
text fields as formulas.

## Verification recorded on November 12

- `cargo test --workspace --locked`: 132 passing tests, including 22 additional security/window tests compared with the previous state.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo audit` 0.23.0 found no known vulnerability after the RSA update. At that time, six unmaintained indirect packages were reported: `bincode`, `pin-utils`, `ttf-parser`, `zbus_names`, `zvariant` and `zvariant_derive`. No advisory was suppressed. The old macOS Slint build tools used `pin-utils`; the other packages were in the lockfile for other/optional platform paths. **Current status:** the egui/font migration removed the first three; the three zbus-related maintenance warnings remain documented in [Testing](testing.md).
- Native macOS verification passed: the updated application started, an existing vault unlocked, and the transfer form was inspected at full width. Window position and size survived macOS zoom, Cmd-Q, closing the window and restarting. The original dimensions were restored afterward.
- No real card-reader test was performed during this review, at the user's request. The DDV change validates envelopes/padding; it does not establish full cryptographic authentication of every DDV bank response. RDH response signatures are verified separately. The later successful RDH-7 test is recorded in [RDH review](rdh-review.md).
- `Zeroizing` limits the lifetime of buffers under application control. It cannot eliminate every temporary copy in GUI, operating-system or third-party code.

## November 14 additions

The PayPal provider uses fixed HTTPS endpoints, disables redirects and allows only
`GetBalance` and `TransactionSearch`. API credentials are separately encrypted in the
vault and re-encrypted during a password change. Error messages expose only validated
numeric API codes, not raw provider text or credentials. Complete retrieval and a
successful encrypted save precede changes to the active vault. Synthetic tests cover
pagination, cancellation, malformed data, scoped updates, password changes and form
clearing. This is not evidence of successful live PayPal authentication.

These source reviews and automated tests do not replace an independent security audit
or banking certification. See [SECURITY.md](../SECURITY.md) for reporting instructions.
