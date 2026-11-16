# Apple signing in GitHub Actions

BankingCraft's macOS release pipeline signs both architectures with the explicitly
configured **Developer ID Application** holder and team, then notarizes them with Apple.
The authorized signing account is **Sven Carstensen**, enrolled as an individual.
The displayed signing identity therefore uses his name; AppCreation GmbH remains the app's publisher.
The bundle identifier remains `de.appcreation.bankingcraft`. This is direct DMG distribution,
not a Mac App Store upload. The existing local build command continues to use ad-hoc signing.

## One-time Apple setup

Use the authorized **Sven Carstensen** team in the [Apple Developer account](https://developer.apple.com/account/).
Copy its ten-character **Team ID** from Membership details and its exact certificate-holder
name. The account needs an active Apple Developer Program membership and accepted agreements.

1. Create or select a **Developer ID Application** certificate for that team.
   In Xcode, use Settings → Accounts → the authorized team → Manage Certificates,
   or use Apple's Certificates, Identifiers & Profiles portal. Certificate creation may require
   the Account Holder or an authorized Admin. Cloud-managed certificates without an exportable
   private key cannot be used by this pipeline.
2. Export the certificate **with its private key** from Keychain Access as a password-protected
   `.p12`. An Apple Distribution, Mac Distribution or Developer ID Installer certificate is
   not a substitute. Never export an unrelated account's certificate.
3. In [App Store Connect](https://appstoreconnect.apple.com/), select the same account,
   then Users and Access → Integrations → App Store Connect API. Create a **team API key**
   with access to notarization (Developer role), download its `.p8`, and record its Key ID
   and Issuer ID. The private key can only be downloaded once. This setup deliberately uses
   a team key and requires its issuer; it does not use an Apple ID password or interactive 2FA.

For this app's current capabilities, Developer ID distribution does not need a provisioning
profile or sandbox entitlements. All embedded native code is signed with the same identity;
library validation remains enabled. Review this if restricted Apple capabilities are added.

## Configure the GitHub repository

Repository: [SvenCarstensen/BankingCraft](https://github.com/SvenCarstensen/BankingCraft).
Use Settings → Secrets and variables → Actions, or the setup helper below.

Configured on 2026-11-16 for **Sven Carstensen**, team `Y322FFQ9N6`:
all five secrets and both variables below are installed. The Developer ID Application
certificate expires on **2027-12-16**. The dedicated team API key is named
**BankingCraft GitHub Notary** and has the Developer role. Local credential validation
confirmed both the certificate/private key and Apple's notarization API authentication.
The complete workflow on `master` has also passed for Apple Silicon and Intel: Apple accepted
both the app and DMG for each architecture, and their stapled tickets, Gatekeeper assessments
and packaged startup checks passed. This signing setup is used for release 0.1.0 and later.

| Kind | Name | Value |
| --- | --- | --- |
| Variable | `APPLE_TEAM_ID` | Verified ten-character ID of the authorized signing team |
| Variable | `APPLE_SIGNING_NAME` | Exact certificate-holder name (`Sven Carstensen` for the selected account) |
| Secret | `APPLE_CERTIFICATE_BASE64` | Base64 of the complete `.p12` export |
| Secret | `APPLE_CERTIFICATE_PASSWORD` | Password protecting that export |
| Secret | `APPLE_API_KEY_ID` | App Store Connect team API Key ID |
| Secret | `APPLE_API_ISSUER_ID` | Issuer UUID from the same signing team |
| Secret | `APPLE_API_PRIVATE_KEY` | Complete original `.p8` text, including PEM header/footer and newlines |

On a Mac with Xcode and authenticated GitHub CLI, run this from the repository,
replacing the example paths and identifiers:

```sh
python3 scripts/configure-apple-signing.py \
  --repo SvenCarstensen/BankingCraft \
  --certificate /absolute/path/BankingCraft-Developer-ID.p12 \
  --api-key /absolute/path/AuthKey_KEYID.p8 \
  --team-id APPLE_TEAM_ID \
  --signing-name 'Sven Carstensen' \
  --key-id API_KEY_ID \
  --issuer-id API_ISSUER_UUID
```

The helper prompts for the `.p12` password without echo, validates the configured holder's
signing identity and Apple's notarization API access, then sends secrets through standard
input to `gh secret set`. It never prints secret values or includes them in command arguments
to GitHub. It creates or replaces these five repository secrets and the two identity variables;
if a GitHub write fails, correct the access issue and rerun the helper to complete the set.
An API key from another team will not authorize the final notarization;
the signed dry run below is the final account-integration check.

Keep the original exports in secure storage outside the checkout. `.p12`, `.p8` and
keychain files are ignored by Git. Do not paste their contents or passwords into issues,
pull requests or chat. Rotate an expiring certificate by rerunning the setup with its
replacement and testing a new signed package before revoking the old certificate.

## Validate and release

For credential rotation or future releases:

1. Run **Apple signing check** in Actions. This checks that the `.p12` includes a valid
   private key, the certificate is exactly the configured Developer ID Application identity
   with the configured team, a tiny compiled probe can actually be signed and verified,
   and the API credentials authenticate successfully. Signed release runs also perform
   this check before building the app.
2. Run **Release packages** on `master` with `publish=false` and `sign_macos=true`.
   Both architectures must pass all existing tests, signing, notarization, stapling,
   Gatekeeper assessment and the startup test from the final DMG. This produces artifacts
   without publishing a release. Download and launch a package on a separate Mac as well.
3. Update the version as described in [Desktop releases](releases.md), then push its version
   tag or run the workflow with `publish=true`. Tag and publish runs always require signing,
   even if `sign_macos` was unchecked. Missing credentials or an Apple rejection fail the job;
   no unsigned fallback can reach the release publisher.

An explicit `sign_macos=false`, `publish=false` manual **branch** run remains available
for ad-hoc test packages without Apple credentials. It does not create a GitHub release.
Pull-request CI runs signing regression tests with synthetic data and no Apple secrets.

## What happens on the runner

After compilation and cache saving, packaging imports the `.p12` into a dedicated temporary
keychain. A random keychain password is generated per run. Only signing tools receive access;
the keychain is temporarily prepended to the search list, preserving existing entries.
The original search list is restored during cleanup. Credentials are scoped to the signing steps.

The packager finds Mach-O code by file header, including extensionless helpers and `.so`
plugins, signs it inside out with hardened runtime and a secure timestamp, then signs the
app bundle. It verifies every code object's team, certificate authority, timestamp and
runtime flag, then tests the signed native worker and plugins with synthetic data.
Native licenses, XML profiles and other data live in `Contents/Resources/native`.
Relative links within the bundle preserve the libraries' existing resource paths;
the files in code directories are Mach-O code or links, not unsigned data files.

The app is uploaded as a ZIP, accepted by Apple, stapled and assessed by Gatekeeper. The
DMG is then built from the stapled app, signed, notarized and stapled separately so both
the installer image and installed app carry their offline tickets. Only the fully checked
DMG enters `dist/`; the normal startup smoke test runs from that image before artifact upload.
No release build is compiled or signed on a developer's Mac by these workflows.

Apple submission results and logs appear in `notarization-macos-aarch64` and
`notarization-macos-x86_64` Actions artifacts for seven days, separate from release assets.
Each submission is uploaded once and its ID is saved before polling for up to 30 minutes;
the packaging job allows 120 minutes in total. Temporary network failures during credential
validation, timestamp signing, status/log retrieval and stapling get up to four attempts,
with 10, 30 and 60 seconds between attempts. Each request is limited to 120 seconds, and
status retries share the submission's 30-minute deadline. Authentication errors, invalid
signatures, rejected submissions and unknown errors stop immediately. The standalone
credential check allows 25 minutes to cover both credential and timestamp retries.

Polling resumes the existing submission ID without uploading again. Uploads have a
10-minute limit and are not automatically replayed if the outcome is ambiguous and no
ID is available. Inspect Apple's submission history before submitting again in that case.
A final timeout still fails the job rather than distributing an unaccepted package.
The `*-submission.json`, `*-status.json` and Apple log artifacts retain the available
evidence; Apple may continue processing after the local wait expires.
Temporary keychains and credential files are removed on success or exception. If a runner
is forcibly stopped, GitHub destroys the hosted VM. Do not switch to self-hosted runners
without adding cancellation-safe cleanup and access isolation.

Release 0.1.0 is the first version distributed through this Developer ID signing pipeline.
Already published `0.0.10` DMGs remain unsigned and their apps remain ad-hoc signed.
Mock tests and credential validation alone do not prove Apple acceptance or Gatekeeper
behavior; verify the signed release workflow before publishing.

## GitHub runner allocation failures

The **Retry runner allocation failures** workflow inspects completed CI, Dependency audit,
Apple signing check and Release packages runs. It retries only GitHub's explicit
`The job was not acquired by Runner of type hosted even after multiple attempts` failure
when no runner was assigned and no step started. Manual cancellations, application/test
failures, unknown infrastructure errors and jobs that exceeded their execution limit
are not retried by this workflow.

There are at most three total attempts, including the initial run and any manual retries.
Retries wait 60 seconds after attempt one and 120 seconds after attempt two. Only the
current `master` commit (including version tags at that commit) in this repository is
eligible; pull requests and forks are excluded. The workflow rechecks the run and master
after the delay to avoid restarting a superseded or manually restarted run.
If all failed jobs have proven allocation failures, they are retried together. Otherwise,
only one confirmed allocation failure is retried; unrelated failures remain visible.
GitHub also reruns dependent jobs, preserving the original publish/signing inputs.

The controller checks out its trusted default-branch workflow revision, never code or
artifacts from the triggering run. It uses the built-in `GITHUB_TOKEN` with Actions write,
Checks read and Contents read permissions, with no additional secret. Its Actions summary
records the decision. Once the attempt limit is reached, inspect the remaining failure
and rerun the affected job manually when GitHub capacity is available.

## References

- [Apple: Developer ID certificates](https://developer.apple.com/help/account/certificates/create-developer-id-certificates)
- [Apple: Customizing the notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)
- [Apple: Migrating notarization credentials and API keys](https://developer.apple.com/documentation/technotes/tn3147-migrating-to-the-latest-notarization-tool)
- [GitHub: Apple certificates on macOS runners](https://docs.github.com/en/actions/how-tos/deploy/deploy-to-third-party-platforms/sign-xcode-applications)
