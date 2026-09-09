# Release Engineering Plan

Status: repository-local native governance contract, revised 08/09/2026.
Target platform: Windows 11+.

## Validation and security

Frontend CI runs tests, Biome, the build, public HTML formatting, dependency
audit and the in-repository PostEditor parity snapshot. Rust checks, tests and
Clippy run with locked dependencies on GitHub-hosted Windows runners. Do not
run `cargo` or `rustc` on the operator's local machine.

- Secret Scanning.
- Code Scanning with GitHub's official CodeQL Default Setup for Actions,
  JavaScript/TypeScript, and Rust.
- OpenSSF Scorecard SARIF upload for repository posture signals.
- Dependabot alerts.
- Dependabot version updates.
- Private vulnerability reporting.
- Native Code Quality and Dependency Review.
- Zizmor workflow scanning.

Inherited Enterprise protections remain authoritative. Findings must be triaged
using their evidence; provider quota exhaustion is not a code defect. The
historical Rust extractor investigation is retained in
[CODEQL_RUST_EXTRACTION.md](../.github/CODEQL_RUST_EXTRACTION.md), not as a
requirement to restore the advanced workflow.

Same-repository Dependabot PRs enable native auto-merge against their exact head
SHA, including standalone majors. Minor/patch grouping does not restrict
auto-merge eligibility. Required checks govern admission; there is no merge
queue, custom release-readiness poller, custom SARIF interpreter, tag-policy
interpreter or central controller.

## GitHub Releases

Versioning convention:

- App and changelog labels use `vX.X.X`; protected Git tag refs use the
  zero-padded `vXX.YY.ZZ` form.
- `package.json` stores the numeric semver core, for example `0.5.58`; the
  corresponding release tag is `v00.05.58`.
- Keep `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
  synchronized for a product version, including applicable dependency-manager
  lockfile metadata. Update `CHANGELOG.md` for the change actually delivered.
- A governance-only change does not itself require a new product version.
  Testing the complete publication path requires a new synchronized version
  and approval of its release PR; an existing published version is a no-op.

Release readiness requires:

- Clean working tree.
- Passing frontend and Windows Rust CI and applicable native security checks.
- No credentials or secret values in tracked files; necessary nonsecret
  configuration identifiers are permitted.
- No private protocol, draft, evidence cache, or transcript committed.
- Updated `CHANGELOG.md`.
- Updated README and security docs when behavior changes.
- Release tag pointing to the exact protected `main` source commit.
- GitHub-generated release notes for the shipped changes, with the distribution
  format and operational limitations documented here and in the bundled
  `LEIAME.md` and `PORTABLE-RUN.txt`.
- After a finalized version is delivered, delete local `src-tauri/target` from `C:\Users\leona\lcv-workspace\maestro-app` to keep the workspace lean. Perform this only after validation/release closure and only after verifying the resolved absolute path is under `maestro-app\src-tauri\target`.

Distribution policy:

- GitHub Releases is the primary human-facing distribution channel.
- Windows releases preserve archive name
  `maestro-editorial-ai-<tag>-windows-x64-portable.zip`. The runner stages files
  in `release/Maestro-Editorial-AI-<tag>-windows-x64-portable`, but that staging
  directory is not included inside the ZIP: its contents are archived flat at
  the ZIP root, using `Compress-Archive -Path "$stage/*"`. This preserves the
  existing extract-and-run layout. See Microsoft's
  [archive path semantics](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.archive/compress-archive#example-4-compress-a-directory-that-excludes-the-root-directory).
- Each archive contains `Maestro Editorial AI.exe`, `LEIAME.md`, `LICENSE`,
  `NOTICE`, `THIRDPARTY.md`, `THIRD-PARTY-NOTICES.txt`, `CHANGELOG.md` and
  `PORTABLE-RUN.txt`. `SHA256SUMS.txt` remains a release asset.
- The installed official Tauri CLI builds with
  `tauri build --ci --no-bundle -- --locked`; native PowerShell commands package
  and hash the output. The workflow does not create an MSI, NSIS installer or
  NuGet package. See [Tauri distribution](https://v2.tauri.app/distribute/).
- A new synchronized version on protected `main` authorizes publication.
  Manual execution also targets `main`; no tag self-dispatch is used. Native
  job dependencies order Windows packaging, GitHub Release publication and
  the GHCR mirror.
- Existing published GitHub Releases and their assets are never overwritten. Native
  `gh release` commands publish final assets against the exact source commit;
  see [GitHub CLI release creation](https://cli.github.com/manual/gh_release_create)
  and [immutable releases](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases).
- Beta tags are published with GitHub's prerelease flag and the GHCR `beta`
  channel; they never replace the stable `latest` release or package channel.
- A new portable archive receives GitHub build-provenance attestation in the
  Windows build job. Published assets are also checked with the native
  [`gh release verify-asset`](https://cli.github.com/manual/gh_release_verify-asset)
  command before subsequent publication steps proceed.

### Existing versions and failed-job recovery

A fresh push-triggered or manual run whose synchronized version is already a
published GitHub Release succeeds without rebuilding or republishing it. Its
publication jobs are skipped: no new ZIP, GHCR mirror run or Linear release is
produced. Use the [GitHub Releases list](https://github.com/LCV-Ideas-Software/maestro-app/releases)
to distinguish published versions from the current source version. An approved
new version exercises Windows publication, GHCR mirroring and Linear sync;
success of the existing-version no-op does not validate those publication jobs.

If a publication run fails after earlier jobs succeeded, recover through
**Actions -> Release -> the original run -> Re-run jobs -> Re-run failed jobs**,
or use the official CLI against that same run:

```powershell
gh run rerun RUN_ID --failed --repo LCV-Ideas-Software/maestro-app
```

This recovery keeps the original successful prerequisite jobs and their outputs
instead of recomputing `prepare`; artifact downloads use the successful build's
artifact ID. GitHub reruns retain the original event's commit SHA, ref and actor
privileges. A publication retry verifies already-published immutable assets;
an owned draft may have its unfinished assets replaced before publication.
**Re-run all jobs** or a new manual dispatch is not equivalent recovery: once
publication exists, recomputing `prepare` follows the no-op path. Do not delete
or move a release/tag to force the workflow forward. Recovery requires the
original run and its needed artifacts to remain available.

See GitHub's [failed-job recovery documentation](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/re-run-workflows-and-jobs#re-running-failed-jobs-in-a-workflow)
and [`gh run rerun`](https://cli.github.com/manual/gh_run_rerun).

Build-provenance attestation is not Windows Authenticode signing. This contract
does not introduce an application updater, signing certificate or third-party
signing service. The app remains portable, with runtime data under its ignored
local `data/` directory.

The full `THIRD-PARTY-NOTICES.txt` bundle and static vendor license texts under
`scripts/legal/` remain preserved from the pre-reform snapshot. The retired
custom legal generator and verifier are not replaced with another custom gate.
Maintenance of distribution notices remains required; official automation for
that work is tracked separately in GIT-213. The reform does not regenerate or
recertify the snapshot, nor certify it for later dependency changes.

GitHub settings changes, including required-check rules or environment removal,
require the operator's explicit prior approval. Preparing this workflow does
not itself authorize administrative changes or certify their deployment.

## GitHub Packages

GitHub Packages is enabled through GHCR/OCI publishing in `.github/workflows/release.yml`.

Package policy:

- No NuGet package is used for Maestro's Windows app distribution.
- The package is an OCI mirror of the same Windows portable ZIP published to GitHub Releases.
- The package name is `ghcr.io/lcv-ideas-software/maestro-app-windows-portable`.
- Human users should use GitHub Releases; GitHub Packages is for automation, provenance, and machine retrieval.
- GHCR publishes the exact tag plus `latest` for stable releases or `beta` for
  prereleases. The versioned OCI mirror may be pushed again during failed-job
  recovery, using the same already-published, verified ZIP; GHCR tag immutability
  is not claimed. Channel tags update only if this release is still the current
  stable release or newest published prerelease, so an older retry does not move
  a channel backwards.

Future package surfaces, such as npm packages for shared schemas, require a separate approval before publishing.

## Linear release integration

The same-repository reusable `.github/workflows/linear-release.yml` runs only
after a new GitHub Release and its GHCR mirror have been published successfully.
It uses the official Linear action, the existing `linear-release` environment
and `LINEAR_ACCESS_KEY`; no extra token or repository controller is introduced.

## GitHub Sponsors

Sponsors support is active through `.github/FUNDING.yml`, with
`github: LCV-Ideas-Software` as the current sponsor recipient and the Maestro
organization GitHub Pages URL as the custom funding link.

## GitHub Pages

GitHub Pages uses the modern GitHub Actions source, not the legacy `gh-pages`
branch. The public support page lives in `site/`; PRs build its Pages artifact,
while only `main` pushes or manual `main` execution deploy it using official
Pages actions and the `github-pages` environment. A fresh fork must enable **Settings -> Pages ->
GitHub Actions** once before its first deployment; the workflow does not request
an administrative credential to self-enable Pages.

## CodeQL Mode

CodeQL Default Setup already covers Actions, JavaScript/TypeScript and Rust on
standard GitHub-hosted runners. Current native Rust analysis succeeded without
a repository-configured sysroot; see the exact-head evidence in
[CODEQL_RUST_EXTRACTION.md](../.github/CODEQL_RUST_EXTRACTION.md).
Enterprise-native code-scanning protection is authoritative; the repository
does not maintain an advanced workflow, custom SARIF parser or duplicate gate.

OpenSSF Scorecard is a separate repository-posture scanner. Its alerts must be triaged by rule:

- workflow permission findings should be fixed in YAML;
- dependency findings that come from target-inactive transitive lockfile entries should be documented in `docs/dependabot-alert-triage.md` and, when OSV supports it, in an adjacent scanner config;
- organizational signals such as branch-protection tier, human code-review ratio, fuzzing integration, and OpenSSF Best Practices badge require an explicit policy decision before changing repository rules or dismissing alerts.

## Historical pre-public audit checklist

The repository is already public. The checklist used for a private-to-public
transition is retained as historical guidance, not a new publication gate:

- Run full-history secret scanning.
- Run current-tree secret scanning.
- Verify `.gitignore` excludes runtime state.
- Verify no private protocol contents, OneDrive documents, drafts, evidence caches, logs, or CLI transcripts are tracked.
- Review GitHub Actions logs for accidental disclosure.
- Review package metadata, README, screenshots, release notes, and fixtures for private data.
