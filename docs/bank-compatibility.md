# German bank interoperability review

Reviewed on **2026-11-13**. This is a source/configuration review and an offline
protocol test record, **not a list of banks successfully tested with BankingCraft**.
Only Postbank PIN/TAN and VR Bank Nord RDH-7 retrieval have been confirmed live.
BankingCraft's own FinTS product registration is still pending. A valid registered
product ID and bank-enabled credentials are prerequisites for further live tests.

## Sources and precedence

GnuCash uses AqBanking for FinTS. The comparison used the
[AqBanking bank table](https://www.aquamaniac.de/rdm/projects/aqbanking/wiki/AqBanking6_Bankentabelle)
and AqBanking source commit `33504e378993d9cdb6702bd90c7039ef378fe31f`:

- `banking/provider_job.c`: choose CAMT/MT940 using preferences and supported jobs.
- `dialogs/dlg_pintan.c`: an empty customer-ID input defaults to the user ID on the wire.
- `ajobs/jobgettransactions.xml` and `jobgettrans_camt.xml`: statement parameters, including `StoreDays`.
- `msglayer/dialog_https.c`: accept base64 or raw FinTS HTTP responses.
- `applayer/cbox_voptan.c`: TAN/decoupled approval and explicit SCA exemptions.

These behaviors were implemented in Rust; no GnuCash product ID, credentials or
private configuration were imported. Current bank publications take precedence
over old wiki entries. An HTTP endpoint is not evidence of available account
permissions, TAN methods, successful authentication or payment support.

## Bank/source matrix

All rows below remain **live-unverified in BankingCraft**, except the two
explicitly confirmed connections in the README. Endpoints refer to PIN/TAN unless
stated otherwise; regional bank codes must be selected from the directory.

| Bank/group | Reviewed configuration and practical requirements | Evidence / limits |
|---|---|---|
| ING | `https://fints.ing.de/fints/`; username/access number and banking password; customer-ID input may stay empty. Read-only. Current-account history is limited to 90 days; periodically authenticate in the bank's own online banking. | [ING FinTS help](https://www.ing.de/hilfe/log-in/fints/). The hardcoded history fallback is restricted to current/unspecified account types; BPD limits apply to other types. |
| DKB | `https://fints.dkb.de/fints`; online banking username, customer-ID input empty; DKB app (decoupled), manual chipTAN or chipTAN QR. | [DKB electronic banking](https://www.dkb.de/geschaeftskunden/electronic-banking). This official source is for business customers; retail/account-specific availability still needs confirmation. Removed the obsolete classic-card host from the directory. |
| Commerzbank | `https://fints.commerzbank.de/fints`; participant number/username; separate FinTS/photoTAN enablement may be required. Prefer negotiated CAMT. | [Server addresses](https://www.commerzbank.de/service/kommunikationsadressen-ports-und-oeffentlicher-bankschluessel-fuer-hbci/), [HBCI format migration](https://www.commerzbank.de/service/umstellung-von-mt-formaten-auf-camt-formate-in-hbci/). The bank scheduled MT940/942 shutdown for 2026-10-14. Enablement is also documented in the AqBanking table. Its RAH key-file entry is **not** implemented by BankingCraft's RDH-card backend. |
| comdirect | `https://fints.comdirect.de/fints`; eight-digit access number and PIN. Automatic format negotiation, with an optional legacy MT940 preference. | [Official FinTS setup page](https://www.comdirect.de/cms/sicherheit-software-banking-fints.html), [login FAQ](https://www.comdirect.de/faq/Was-sind-Zugangsdaten). Search-index content confirmed the endpoint/login; the full FinTS page returned HTTP 401 during review. The older AqBanking recommendation to disable CAMT is **not imposed automatically**. The exact current format rollout remains to be verified live. |
| Deutsche Bank | `https://fints.deutsche-bank.de/`; Deutsche Bank ID or branch/account identifier, current password; photoTAN/BestSign. Refresh after migration. Omit an optional photoTAN medium name. | [Current bank FinTS FAQ](https://www.deutsche-bank.de/pk/konto-und-karte/services/hbci-fints.html). The old wiki restriction to less than 90 days is obsolete: the bank describes history back to 2024-09-01, with additional approval where required. It also documents ten-digit account numbers for certain account-reference errors; no arbitrary account-number rewrite is applied. |
| norisbank | `https://fints.norisbank.de/`; new norisbank ID or branch/account number and current password; same customer/user identifier; photoTAN or BestSign. | [Current bank FinTS FAQ](https://www.norisbank.de/service/banking/sicherheitsverfahren/hbci-verfahren.html). Old mobileTAN advice is obsolete. A changed bank identity/password must be updated by the user. |
| Postbank | Existing `https://hbci.postbank.de/banking/hbci.do` profile and Postbank ID/device-name handling retained. | Existing confirmed PIN/TAN retrieval and offline regression coverage; no new live test in this review. |
| Sparkassen | Use each institution's own directory URL and bank code, online banking name/Legitimations-ID, bank-advertised pushTAN/chipTAN. | [Sparkasse Nordschwaben communication parameters](https://www.sparkasse-nordschwaben.de/fi/home/produkte/banking-und-software/kommunikationsadressen.html). This verifies the setup pattern, **not every regional directory entry**. Classic HBCI cards and chipTAN are different access methods. |
| Volks-/Raiffeisenbanken and PSD | Regional `fints1.atruvia.de` or `fints2.atruvia.de`, NetKey/alias, bank-advertised TAN methods. Regional host assignment is preserved. | [Genobank Unterallgäu FinTS](https://www.genobank-unterallgaeu.de/banking-und-vertraege/banking/banking-firmenkunden/fints-ebics.html), [PSD München external software](https://www.psd-muenchen.de/banking/externe-finanzsoftware.html). The confirmed VR Bank Nord RDH connection is not evidence for all Atruvia banks/security methods. |
| Sparda-West | All four directory BLZ entries now use `https://fints2.atruvia.de/cgi-bin/hbciservlet`; current Sparda-NetKey/alias and PIN, initial bank-web login/activation after migration. | [Bank migration FAQ](https://www.sparda-west.de/banking-und-service/online-banking/fragen-und-antworten.html): effective 2025-10-13. The [Bundesbank entry for 40060560](https://www.bundesbank.de/action/de/747634/bbkpspsearch?id=024793) confirms former Sparda Münster belongs to West. Three obsolete URLs and that name were corrected. |
| Other Sparda banks | Existing Atruvia-2 entries checked for München, Hessen, Hannover, Nürnberg, Baden-Württemberg and Südwest. Credentials must match the regional migration. | [München](https://www.sparda-m.de/banking-und-vertraege/digitales-banking/onlinebanking/faq-onlinebanking.html), [Hessen](https://www.sparda-hessen.de/hilfe-und-service/fragen-und-antworten.html), [Hannover](https://www.sparda-h.de/service/faq-fragen-und-antworten/online--und-mobile-banking.html), [Nürnberg](https://www.sparda-n.de/it-wechsel/aktuelle-informationen/ueberblick.html), [Baden-Württemberg migration brochure](https://hub.sparda-bw.de/storyblok/asset/x/451f95e076/english_future-banking-sparda-bank-bw.pdf?_hash=nafdt4OLtUV46J7_dKTz7AML1zl0K-xZLchSIoeUxos), [Südwest](https://www.sparda-sw.de/privatkunden/service/fragen-und-antworten/sparda-online-banking.html). The generic Sparda website still describes an older server; it was not used to override regional guidance. |
| 1822direkt | `https://fints.1822direkt.com/fints/hbci`; access BLZ 50050222, online identifier and PIN. Keep account BLZ separate from login BLZ. | [Bank setup guide](https://www.1822direkt.de/fileadmin/Home/Dokumente/PDF/Service/Einrichtung_PIN_TAN_in_HBCI.pdf). The guide is older; the available TAN method must come from current bank parameters. |
| apoBank | Existing `https://fints.apobank.de/FinTs30PinTanHttpGate`, apoKennung/alias. Removed the obsolete Atruvia card-host suggestion. | AqBanking table and [SFirm migration guide](https://www.sfirm.de/fileadmin/Downloads/Support/PDFs/SFirm_Kundenleitfaden_apoBank_HBCI-Umstellung.pdf). **No fresh first-party bank endpoint confirmation found**; the sources date from the 2020 migration/2022 review. No automatic legacy `noBase64` request mode enabled. |
| HypoVereinsbank | Existing `https://hbci-01.hypovereinsbank.de/bank/hbci`; use central PIN/TAN BLZ 70020270 and the ten-digit Direct Banking number. | [StarMoney setup documentation, updated 2026-11-03](https://hilfe.starmoney.de/hc/de/articles/360018751193-Kontoeinrichtung-der-HVB-HypoVereinsbank) confirms login/BLZ. [Bank welcome guide](https://www.hypovereinsbank.de/content/dam/hypovereinsbank/privatkunden/pdf/services/welcome_onlinebanking.pdf) covers approval methods. Endpoint still comes from the directory; no fresh public production-URL confirmation found. The bank's PayGate manual shows a QA intranet URL, which was deliberately not copied. |
| Consorsbank | Existing `https://brokerage-hbci.consorsbank.de/hbci`; account number plus three-digit authorised-user suffix, TAN on login. | [Bank setup FAQ](https://www.consorsbank.de/web/Wissen/FAQ/kontokarten/HBCI-Schnittstelle-Konto-und-Zahlungsverkehrsprogramm) currently says FinTS **4.0 or later**. BankingCraft implements **3.0**, so compatibility remains unresolved; the directory entry must not be presented as working support. |

The wider embedded directory contains over 2,000 entries. This review does not
certify all of them as current. Renault Bank's Atruvia-1 entry was retained: the
AqBanking table suggests Atruvia-2, but no bank source resolving that discrepancy
was found. Unsupported protocols, securities trading, key-file RAH and account
permissions cannot be added by copying an endpoint.

## Implemented behavior

- New access setup explains bank-specific login requirements. Statement-format
  preference is stored in the encrypted vault; old vaults default to automatic
  CAMT selection. A preference cannot enable a job absent from BPD/UPD. A failed
  request is not replayed in a different format.
- Known retired ING, DKB and GAD/Fiducia **HTTPS** URLs are mapped by exact
  host/path to documented successors. Custom endpoints and TCP card connections
  are preserved. Sparda migration changes credentials as well, so existing
  accesses must be set up with the new identifiers rather than blindly replayed.
- Fresh BPD replace cached capabilities, including retired TAN methods/formats.
  Updated clients request the new parameter fields once for older vaults.
- BPD `StoreDays` and ING's current-account limit bound retrieval. A shortened
  history generates a visible warning. Older local transactions remain stored;
  an entirely unavailable requested interval fails instead of becoming an empty
  successful download.
- Missing BICs are filled only from the exact account BLZ in a valid German IBAN.
  Bank-supplied BICs win. Conflicting account identifiers are not guessed. HISPA
  enrichment works regardless of HIUPD ordering and does not cross-match a
  different IBAN solely because its account number happens to match.
- Raw FinTS HTTPS responses preserve binary/Latin-1 content. TLS validation,
  redirect prohibition, size limits and message/authentication checks remain in
  force. No bank-signature verification bypass was imported from upstream flags.
- ING payment/standing-order mutations fail before requesting credentials or
  contacting the bank. Other payment operations retain existing capability,
  payee-verification and approval checks.

## Verification

Run `./scripts/test-business.sh`. Tests use scripted responses and synthetic data,
never real accounts. Added cases cover format selection and permissions across
eight endpoint families, history limits/warnings, ING write rejection, Deutsche
Bank photoTAN device-name handling, BPD replacement, safe HISPA/BIC enrichment,
known URL migration, old-vault defaults, raw/base64 response decoding and the
native setup-form format selection. Existing Postbank, RDH, pagination, payment
approval and no-retry tests remain part of the suite.

Review result: **227 tests passed**, three manual screenshot-export tests ignored;
`cargo clippy --workspace --all-targets --locked -- -D warnings` passed. No live
bank authentication or payment submission was performed for this review.

Bank publications can change independently of a BankingCraft release. A further
bank should be labelled live-tested only after successful account, balance and
transaction retrieval with that bank's real enabled access, recording the date,
TAN/card method and tested scope. Payment submission needs its own separate test.
