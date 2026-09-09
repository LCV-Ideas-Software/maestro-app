# Security Policy

## Supported status

The [latest published stable Windows release](https://github.com/LCV-Ideas-Software/maestro-app/releases/latest)
is supported. The source version in `package.json` may be ahead of that release.
The current `main` branch remains supported for security fixes. Repository
automation uses native GitHub controls and SHA-pinned official Actions with
least-privilege job tokens. CodeQL Default Setup analyzes Actions,
JavaScript/TypeScript and Rust; the separate Windows CI job runs the Rust
compiler, tests and Clippy on GitHub-hosted runners, never on the operator's
local machine.

Same-repository Dependabot PRs enable native GitHub auto-merge for their exact
head SHA, including major updates, subject to required checks and inherited
Enterprise protections. There is no merge queue, central controller, custom
SARIF interpreter or custom release-readiness poller.

The repository-local release workflow publishes a new synchronized version
from protected `main`, using native job dependencies for the Windows portable
archive, immutable GitHub Release and GHCR mirror. Published GitHub Releases
and their assets are never overwritten; the GHCR version tag may be pushed
again with the same verified published ZIP during failed-job recovery. A fresh
run for an already-published version creates no new archive or Linear release.
Recovery uses the original run's native **Re-run failed jobs**, not deletion or
retagging of a published release; see the
[release recovery contract](./docs/release-engineering-plan.md#existing-versions-and-failed-job-recovery).
Full bundled notices, SHA-256 checksums and GitHub
build-provenance attestation are preserved. These attestations are not a claim
of Windows Authenticode signing or a configured application updater. The
official Linear integration runs after successful publication through the
same-repository reusable workflow and existing `linear-release` environment.

## Reporting a vulnerability

Please do not open a public issue for suspected vulnerabilities, credential leaks, private data exposure, authentication bypasses, payment-flow issues, supply-chain issues, or deployment misconfiguration.

Report privately by email:

- security@lcv.dev

If GitHub private vulnerability reporting is enabled for this repository, that channel is also acceptable.

Please include:

- affected repository, component, route, package, workflow, or public surface;
- affected version, release tag, commit SHA, or deployment URL when known;
- impact and exploitability;
- reproduction steps or a safe proof of concept, if available;
- whether any credential, personal data, payment data, private editorial material, or operational secret may be involved.

## Scope

In scope: application code, Workers/Pages functions, package publication, GitHub Actions, dependency and supply-chain configuration, repository publication boundaries, security documentation, and public service configuration documented in this repository.

Out of scope: social engineering, physical attacks, denial-of-service testing without prior written authorization, spam, automated noisy scanning, and reports that rely only on outdated browser or dependency versions without a concrete vulnerable path in this repository.

## Coordinated disclosure

LCV Ideas & Software will triage reports privately, request clarification when needed, and coordinate remediation before public disclosure. Public disclosure should wait until a fix or mitigation is available, unless there is an immediate user-safety reason to do otherwise.
