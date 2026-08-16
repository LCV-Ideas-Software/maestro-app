# Release Engineering Plan

Status: planning baseline.
Target platform: Windows 11+.

## GitHub Security Features

Maestro is developed as if these GitHub features are already active:

- Secret Scanning.
- Code Scanning with GitHub's official CodeQL Advanced Setup for Actions,
  JavaScript/TypeScript, and Rust.
- OpenSSF Scorecard SARIF upload for repository posture signals.
- Dependabot alerts.
- Dependabot version updates.
- Private vulnerability reporting.

Security alerts are release blockers until triaged.

## GitHub Releases

Versioning convention:

- App and changelog labels use `vX.X.X`; protected Git tag refs use the
  zero-padded `vXX.YY.ZZ` form.
- `package.json` stores the numeric semver core, for example `0.5.58`; the
  corresponding release tag is `v00.05.58`.
- Every release or scaffold milestone updates `CHANGELOG.md` under a concrete `vX.X.X` heading before Commit & Sync.

Release readiness requires:

- Clean working tree.
- Passing CI and all three official CodeQL Advanced Setup analyses.
- No secret-shaped strings in tracked files.
- No private protocol, draft, evidence cache, or transcript committed.
- Updated `CHANGELOG.md`.
- Updated README and security docs when behavior changes.
- Protected tag ref pointing to the GitHub-verified `main` commit.
- GitHub Release notes that identify installer status, Windows 11+ target, portable layout, checksums, and known limitations.
- After a finalized version is delivered, delete local `src-tauri/target` from `C:\Users\leona\lcv-workspace\maestro-app` to keep the workspace lean. Perform this only after validation/release closure and only after verifying the resolved absolute path is under `maestro-app\src-tauri\target`.

Distribution policy:

- GitHub Releases is the primary human-facing distribution channel.
- Windows releases are ZIP archives containing the portable executable, license, README, changelog, and checksum.
- The release workflow uses `tauri build --no-bundle`; it does not create an MSI, NSIS installer, or NuGet package.
- A synchronized version change on protected `main` authorizes the release
  workflow to create the corresponding protected `vXX.YY.ZZ` or
  `vXX.YY.ZZ-betaN` tag and dispatch the tag-bound publication. Manual recovery
  is restricted to an existing protected tag with an exactly matching input.
- Beta tags are published with GitHub's prerelease flag and the GHCR `beta`
  channel; they never replace the stable `latest` release or package channel.
- Portable archives receive GitHub artifact attestation when the release workflow runs.

## GitHub Packages

GitHub Packages is enabled through GHCR/OCI publishing in `.github/workflows/release.yml`.

Package policy:

- No NuGet package is used for Maestro's Windows app distribution.
- The package is an OCI mirror of the same Windows portable ZIP published to GitHub Releases.
- The package name is `ghcr.io/lcv-ideas-software/maestro-app-windows-portable`.
- Human users should use GitHub Releases; GitHub Packages is for automation, provenance, and machine retrieval.
- GHCR publishes the exact tag plus `latest` for stable releases or `beta` for
  prereleases.

Future package surfaces, such as npm packages for shared schemas, require a separate approval before publishing.

## GitHub Sponsors

Sponsors support is active through `.github/FUNDING.yml`, with
`github: LCV-Ideas-Software` as the current sponsor recipient and the Maestro
organization GitHub Pages URL as the custom funding link.

## GitHub Pages

GitHub Pages uses the modern GitHub Actions source, not the legacy `gh-pages`
branch. The public support page lives in `site/` and is deployed by
`.github/workflows/pages.yml`. A fresh fork must enable **Settings -> Pages ->
GitHub Actions** once before its first deployment; the workflow does not request
an administrative credential to self-enable Pages.

## CodeQL Mode

CodeQL stays on the repository's official Advanced Setup because the supported
matrix includes Actions, JavaScript/TypeScript, and Rust with an explicit Rust
sysroot. Enterprise-native code-scanning protection is authoritative; the repo
does not add a custom SARIF parser or duplicate legacy analysis category.

OpenSSF Scorecard is a separate repository-posture scanner. Its alerts must be triaged by rule:

- workflow permission findings should be fixed in YAML;
- dependency findings that come from target-inactive transitive lockfile entries should be documented in `docs/dependabot-alert-triage.md` and, when OSV supports it, in an adjacent scanner config;
- organizational signals such as branch-protection tier, human code-review ratio, fuzzing integration, and OpenSSF Best Practices badge require an explicit policy decision before changing repository rules or dismissing alerts.

## Pre-Public Audit

Before changing the repository from private to public:

- Run full-history secret scanning.
- Run current-tree secret scanning.
- Verify `.gitignore` excludes runtime state.
- Verify no private protocol contents, OneDrive documents, drafts, evidence caches, logs, or CLI transcripts are tracked.
- Review GitHub Actions logs for accidental disclosure.
- Review package metadata, README, screenshots, release notes, and fixtures for private data.
