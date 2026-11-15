# German bank interoperability review

Reviewed on **2026-11-13**, with an additional source review on **2026-11-14**. This is a source/configuration review and an offline
protocol test record, **not a list of banks successfully tested with BankingCraft**.
The current AqBanking/Libchipcard runtime and its November 15, 2026 live checks are
documented in [native banking](native-banking.md): Postbank PIN/TAN retrieval and a
VR Bank Nord RDH refresh with a cyberJack. The matrix below records the earlier
endpoint review; it is not a new validation of every bank against the native runtime.
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

The former Rust protocol implementation has been removed. AqBanking now owns these
protocol decisions; the earlier source comparison does not imply that every previous
application-specific policy still applies. Current bank publications take precedence
over old wiki entries. An HTTP endpoint is not evidence of available account
permissions, TAN methods, successful authentication or payment support.

## Bank/source matrix

All rows below remain **live-unverified in BankingCraft**, except the two
explicitly confirmed connections in the README. Endpoints refer to PIN/TAN unless
stated otherwise; regional bank codes must be selected from the directory.

| Bank/group | Reviewed configuration and practical requirements | Evidence / limits |
|---|---|---|
| ING | `https://fints.ing.de/fints/`; username/access number and banking password; customer-ID input may stay empty. Read-only. Current-account history is limited to 90 days; periodically authenticate in the bank's own online banking. | [ING FinTS help](https://www.ing.de/hilfe/log-in/fints/). The native runtime follows AqBanking and bank-reported capabilities; the former hardcoded history policy is no longer present. |
| DKB | `https://fints.dkb.de/fints`; online banking username, customer-ID input empty; DKB app (decoupled), manual chipTAN or chipTAN QR. | [DKB electronic banking](https://www.dkb.de/geschaeftskunden/electronic-banking). This official source is for business customers; retail/account-specific availability still needs confirmation. Removed the obsolete classic-card host from the directory. |
| Commerzbank | `https://fints.commerzbank.de/fints`; participant number/username; separate FinTS/photoTAN enablement may be required. Prefer negotiated CAMT. | [Server addresses](https://www.commerzbank.de/service/kommunikationsadressen-ports-und-oeffentlicher-bankschluessel-fuer-hbci/), [HBCI format migration](https://www.commerzbank.de/service/umstellung-von-mt-formaten-auf-camt-formate-in-hbci/). The bank scheduled MT940/942 shutdown for 2026-10-14. Enablement is also documented in the AqBanking table. Its RAH key-file entry is **not** implemented by BankingCraft's RDH-card backend. |
| comdirect | `https://fints.comdirect.de/fints`; eight-digit access number and PIN. Statement format selection is handled by AqBanking. | [Official FinTS setup page](https://www.comdirect.de/cms/sicherheit-software-banking-fints.html), [login FAQ](https://www.comdirect.de/faq/Was-sind-Zugangsdaten). Search-index content confirmed the endpoint/login; the full FinTS page returned HTTP 401 during review. The older AqBanking recommendation to disable CAMT is **not imposed automatically**. The exact current format rollout remains to be verified live. |
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

## Current integration and verification

AqBanking selects bank jobs and statement formats from the native account
capabilities. BankingCraft does not offer a custom CAMT/MT940 selector. Account
identity, SEPA enrichment, reader access and shared card-session behavior are
specified in [native banking](native-banking.md).

Run `./scripts/test-business.sh`, `python3 scripts/test-native.py` and
`python3 scripts/test-native-adapter.py`. The tests use synthetic data, temporary
vaults and native-library fixtures; they do not authenticate to real banks.
See [testing](testing.md) for current results and the distinction from older
protocol-simulator tests that were removed with the former backend.

Bank publications can change independently of a BankingCraft release. A further
bank should be labelled live-tested only after successful account, balance and
transaction retrieval with that bank's real enabled access, recording the date,
TAN/card method and tested scope. Payment submission needs its own separate test.

## Additional local-source review (2026-11-14)

A supplied static analysis of Bank X 11.1.5 (188), dated November 14, was used as a
lead list, not as an authority for current bank availability. Its 1,801 bank-code
assignments include 1,343 endpoint pairs matching the existing directory after
trimming outer whitespace, 448 differences and 10 previously absent codes.
Most differences concern chip-card hosts; a newly generated extraction can still
contain old configurations. No proprietary module, source implementation, credentials,
product identifier or bulk extracted table was copied into BankingCraft.

Adopted after checking current public sources:

| Change | Evidence |
|---|---|
| Commerzbank BLZ 25040066: add `hbci.commerzbank.de` as the card host, port 3000. The existing HTTPS URL stays unchanged. | [Commerzbank network troubleshooting](https://www.commerzbank.de/service/netzwerkfehler-im-hbci-was-kann-ich-tun/) lists both TCP and HTTPS addresses. This does not add RAH/key-file support. |
| NATIONAL-BANK BLZ 36020030: replace `hbciserver.bankverlag.de` with `fints2.atruvia.de`; explain the new NetKey/PIN and required reinitialization of old RDH keys. | [Bank migration information](https://www.national-bank.de/fileadmin/user_upload/jupiter/pub/index.html) and [ALF-BanCo's migration instructions](https://www.alf-banco.de/aktuelles/national-bank-umstellung/) document the October 2024 migration and TCP host. |
| Warburg setup hints: central BLZ 20120100 and new credentials after the May 2026 migration, including former branch accesses. Marcard setup hints: new online-banking/HBCI credentials. | [Warburg's migration page](https://www.mmwarburg.de/de/bankhaus/neue-it/) and [Marcard's announcements](https://www.marcard.de/de/aktuelles). Already configured Atruvia HTTPS addresses are retained. |
| Trim leading/trailing whitespace when reading directory fields. | Defensive parsing only; whitespace inside an address is never removed to guess a destination. |

Not adopted: old Sparda URLs, the former Marcard/Warburg URLs, Renault's old GAD
URL, an IKB URL containing an internal space, blank endpoints as purported removal
instructions, unconfirmed Deutsche Bank IP addresses and Atruvia region switches,
legacy Santander codes and unknown BLZ 49999924. Existing newer information takes
precedence. BMW/EBICS endpoint strings alone are insufficient to implement those
protocols. Existing TLS checks, message authentication and no-replay behavior remain.
No additional live bank test was performed.

## PayPal: independent read-only provider

The same analysis identified NVP API-signature access. BankingCraft implements
`GetBalance` and `TransactionSearch` independently using PayPal's public references:

- [API signature credentials](https://www.paypal.com/us/cshelp/article/how-do-i-request-an-api-signature-or-certificate-help487): API username/password/signature for eligible Business accounts.
- [GetBalance](https://developer.paypal.com/api/nvp-soap/get-balance-nvp/): **available** funds; `RETURNALLCURRENCIES=1` returns separate currency holdings.
- [TransactionSearch](https://developer.paypal.com/api/nvp-soap/transaction-search-nvp/): UTC date bounds, at most 100 results, signed net amounts and fees. No transaction-class filter is imposed, to avoid excluding bank withdrawals.
- [API error codes](https://developer.paypal.com/api/nvp-soap/errors/): warning 11002 triggers smaller search windows; other failures are surfaced using numeric codes only.
- [PayPal's NVP integration guide](https://developer.paypal.com/upgrade/ec/guide/): fixed live and sandbox signature endpoints. API version 204.0 follows PayPal's [merchant SDK service declaration](https://github.com/paypal/merchant-sdk-php/blob/master/lib/PayPal/Service/PayPalAPIInterfaceServiceService.php).

The network allowlist contains only the two read operations. No PayPal SDK or Bank X
implementation was copied. Search intervals overlap at their boundaries; duplicate
IDs are scoped to currency and transaction type, with conflicting duplicates rejected.
Complete retrieval precedes an atomic vault update. Stable IDs replace changed pending
records while preserving manual categories; old history and unselected wallets remain.
The 2,048-request ceiling, one-second subdivision limit, response size/timeout bounds,
strict parsing and cancellation checks prevent silently accepting partial histories.

The NVP interface is legacy. `GetTransactionDetails`, full invoice/item descriptions,
PayPal payments and arbitrary historical imports are not implemented. Unknown transaction
types/statuses stop the import; non-financial order/authorization/invoice records are
excluded. Available funds are never represented as a booked balance in the UI or camt
export. Analytics remain EUR-only; other currencies are kept separate in balances and
transaction summaries. Live and sandbox authentication have **not** been tested with
real PayPal credentials; automated verification uses synthetic responses only.
