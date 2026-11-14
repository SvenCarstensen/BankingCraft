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

Real payment submission is not yet validated. Bank retrieval has been confirmed
only for the Postbank PIN/TAN and VR Bank Nord RDH-7 setups described in the README.
Automated tests, dependency scans and local code reviews do not constitute an
independent security audit or bank certification.

**Synchronize transactions sends saved orders before retrieving balances and transactions.**
The account or group selection also limits which orders are sent; global synchronization
sends all queued orders first. Sending still requires any authorization requested by the bank.
**Send transfer** submits the current form immediately after confirmation. An interrupted
or uncertain submission requires manual review and is never retried automatically. A failed
or cancelled submission stops synchronization before transaction retrieval.

## Maintainer activation when making the repository public

In **Settings → Advanced Security**, enable **Private vulnerability reporting**
and verify that **Security → Advisories → Report a vulnerability** is available.
Keep reports private; ask reporters to remove accidental personal or banking data.
See [GitHub's setup instructions](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository).
