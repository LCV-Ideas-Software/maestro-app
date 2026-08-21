<p align="center">
  <img src="../../../.github/assets/lcv-ideas-software-logo.svg" alt="LCV Ideas &amp; Software" width="520" />
</p>

# PostEditor Parity Module

[![release](https://img.shields.io/github/v/release/LCV-Ideas-Software/maestro-app?sort=semver)](https://github.com/LCV-Ideas-Software/maestro-app/releases)
[![CI](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/ci.yml/badge.svg)](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/ci.yml)
[![CodeQL](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/codeql.yml/badge.svg)](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/codeql.yml)
[![license: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](../../../LICENSE)

This folder is the Maestro-local compatibility copy of `admin-app/MainSite/PostEditor`.

Hard rule: if Maestro uses TipTap for MainSite content, it must match the PostEditor feature set and persisted HTML contract, not merely use the same editor framework.

## Parity Requirements

- Keep the TipTap extension set aligned with `admin-app/src/modules/mainsite/editor/extensions.ts`.
- Keep Markdown import behavior aligned with `admin-app/src/modules/mainsite/editor/markdownImport.ts`.
- Keep link normalization aligned with PostEditor save behavior.
- Keep figure, image, YouTube, table, task-list, mention, search/replace, slash-command, AI action, and import affordances available.
- Validate generated HTML against the MainSite sanitizer and `mainsite-frontend/PostReader` before enabling direct D1 publish as stable.

## Drift Policy

When `admin-app/MainSite/PostEditor` changes, Maestro must receive the equivalent change or explicitly record why the behavior does not apply. A planned parity check should compare this folder against a reviewed admin-source snapshot and fail CI on unreviewed drift once the public repo has access to that baseline.

## Change History

**Status.** Reviewed compatibility snapshot. Current review: **2026-08-21**. See [CHANGELOG.md](../../../CHANGELOG.md) for the full release history.

The version history at a glance:

| Change             | Notes                                                                                  |
| ------------------ | -------------------------------------------------------------------------------------- |
| 2026-04-26 snapshot | Maestro-local compatibility copy imported from `admin-app/src/modules/mainsite/`.      |
| 2026-08-21 review   | Final allowlist, all non-YouTube link parity, durable draft custody, fixtures and CI drift gate. |

## Source Snapshot

[`parity-snapshot.json`](parity-snapshot.json) is the machine-checked record of the reviewed `admin-app` editor/sanitizer, `mainsite-app` reader, and Maestro compatibility files. `npm run parity:check` fails when a protected local file changes without a reviewed snapshot update.

Maestro intentionally differs from the admin UI only at explicit boundaries: imported HTML receives additional ingress sanitization, Link Integrity blocks unresolved links, and saves enter the portable `mainsite_draft.v1` envelope instead of calling the admin API. Direct D1 publication belongs to MAESTRO-7 and must sanitize independently on the remote side.

## Repository conventions

- **License**: [AGPL-3.0-or-later](../../../LICENSE). Network-service trigger applies: running a modified fork as a public service obligates you to publish modifications.
- **Notices**: see [NOTICE](../../../NOTICE) and [THIRDPARTY](../../../THIRDPARTY.md).
- **Security disclosure**: see [SECURITY.md](../../../SECURITY.md).
- **Code of conduct**: see [CODE_OF_CONDUCT.md](../../../CODE_OF_CONDUCT.md).
- **Changelog**: [CHANGELOG.md](../../../CHANGELOG.md).
- **Contributing**: see [CONTRIBUTING.md](../../../CONTRIBUTING.md).
- **Sponsorship**: see the repo's `Sponsor` button or [central sponsor page](https://www.lcv.dev/sponsor).
- **Action pinning**: all GitHub Actions are pinned by full SHA per supply-chain hardening baseline.
- **Code owners**: [.github/CODEOWNERS](../../../.github/CODEOWNERS).

## Links

- Site: [https://maestro-app.lcv.dev](https://maestro-app.lcv.dev)
- GitHub: [https://github.com/LCV-Ideas-Software/maestro-app](https://github.com/LCV-Ideas-Software/maestro-app)
- Sponsors: [https://github.com/sponsors/LCV-Ideas-Software](https://github.com/sponsors/LCV-Ideas-Software)

## License

AGPL-3.0-or-later. See [LICENSE](../../../LICENSE), [NOTICE](../../../NOTICE), and [THIRDPARTY](../../../THIRDPARTY.md).

---

<p align="center"><span style="font-size: 1.5em;"><strong>Copyright © 2026 LCV Ideas &amp; Software</strong></span><br><sub>LEONARDO CARDOZO VARGAS TECNOLOGIA DA INFORMACAO LTDA<br>Rua Pais Leme, 215 Conj 1713 - Pinheiros<br>São Paulo - SP - CEP 05424-150<br>CNPJ: 66.584.678/0001-77 - IM: 3039854</sub></p>
