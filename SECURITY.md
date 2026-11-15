# Security policy

BankingCraft is an early alpha banking application. Only the latest commit on
`master` is maintained. Older commits and any earlier alpha builds do not receive
separate security backports. There is no production-support or response-time guarantee.

## Report a vulnerability privately

Use GitHub's **Report a vulnerability** button on the
[Security advisories page](https://github.com/SvenCarstensen/BankingCraft/security/advisories)
or [open a private vulnerability report](https://github.com/SvenCarstensen/BankingCraft/security/advisories/new).
The repository maintainer, [SvenCarstensen](https://github.com/SvenCarstensen), handles these reports.

This channel becomes available after the repository is public and private
vulnerability reporting has been enabled. If the button is not available, do not
post vulnerability details in a public issue, discussion or pull request. Wait
until the private reporting channel is available. No email reporting channel is used.

Please include:

- The affected commit or version, operating system and relevant access method.
- Expected and actual behavior, security impact and minimal reproduction steps.
- A small synthetic example or redacted technical diagnostic if needed.

Never include real PINs, TANs, passwords, private keys, product IDs, vault files,
banking exports or unredacted account and transaction data. Use fictional demo
data whenever possible. Do not test by submitting real payments or by accessing
accounts or systems without their owner's permission.

We will assess the report privately, coordinate a correction where possible and
use GitHub advisories to communicate confirmed vulnerabilities and fixes. Please
allow time for this coordination before disclosing exploit details publicly.

## Alpha limitations

Real payment submission is not yet validated. Postbank PIN/TAN account, balance and
transaction retrieval, including credit cards, completed with the AqBanking backend
on macOS on November 15, 2026. Physical card/reader operation remains unvalidated.
Earlier Postbank and VR Bank Nord tests used the previous backend and do not cover
the new integration.
Automated tests, dependency scans and local code reviews do not constitute an
independent security audit or bank certification.

**Synchronize transactions sends saved orders before retrieving balances and transactions.**
The account or group selection also limits which orders are sent; global synchronization
sends all queued orders first. Sending still requires any authorization requested by the bank.
**Send transfer** submits the current form immediately after confirmation. An interrupted
or uncertain submission requires manual review and is never retried automatically. A failed
or cancelled submission stops synchronization before transaction retrieval.

## Current security controls

The native banking process uses bounded private pipes and an in-memory AqBanking
configuration persisted only inside the encrypted vault. PIN/TAN input is not
written to protocol logs. Five-minute idle expiry, system-lock detection, resume
detection and a manual lock hide financial data and revoke outstanding prompts.
The vault uses an exclusive process lock. Payment outcomes that might already
have reached the bank require manual review rather than automatic retry.

A cancelled running job may retain its vault key briefly to persist its final
status. No guarantee is made about erasing all native-library allocations, swap,
OS crash collection, or erasure before the machine suspends. The worker is a
separate process, not an OS sandbox. Read the exact controls, test scope and
external audit checklist in [security hardening](docs/security-hardening.md).

Signed releases, update verification and notarization are deliberately deferred
(point 2 of the requested changes). No independent audit has been commissioned
or completed as part of this implementation.

## Maintainer activation when making the repository public

In **Settings → Advanced Security**, enable **Private vulnerability reporting**
and verify that **Security → Advisories → Report a vulnerability** is available.
Keep reports private; ask reporters to remove accidental personal or banking data.
See [GitHub's setup instructions](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository).
