<p align="center">
  <img src="../../.github/assets/lcv-ideas-software-logo.svg" alt="LCV Ideas &amp; Software" width="520" />
</p>

# Source Protocols

[![release](https://img.shields.io/github/v/release/LCV-Ideas-Software/maestro-app?sort=semver)](https://github.com/LCV-Ideas-Software/maestro-app/releases)
[![CI](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/ci.yml/badge.svg)](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/ci.yml)
[![license: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](../../LICENSE)

Keep private operator-supplied editorial protocols in private storage outside this public repository's checkout, including during planning and testing. This directory documents that boundary; it is not a private staging area.

The existing ignore rules remain a safeguard against accidental staging, not an access-control boundary: a forced add can bypass them. Only explicitly approved, non-confidential public fixtures may be committed. A post-push CI check cannot prevent initial disclosure through a public branch, and Secret Scanning does not classify arbitrary confidential editorial prose.

## Change History

**Status.** Documentation-only confidentiality boundary; private protocols belong outside the checkout. Current release: **not versioned**. See [CHANGELOG.md](../../CHANGELOG.md) for the full release history.

The version history at a glance:

| Change  | Notes                                                                                                  |
| ------- | ------------------------------------------------------------------------------------------------------ |
| Current | Private protocols stay outside the checkout; only explicitly approved non-confidential fixtures are public. |

## Repository conventions

- **License**: [AGPL-3.0-or-later](../../LICENSE). Network-service trigger applies: running a modified fork as a public service obligates you to publish modifications.
- **Notices**: see [NOTICE](../../NOTICE) and [THIRDPARTY](../../THIRDPARTY.md).
- **Security disclosure**: see [SECURITY.md](../../SECURITY.md).
- **Code of conduct**: see [CODE_OF_CONDUCT.md](../../CODE_OF_CONDUCT.md).
- **Changelog**: [CHANGELOG.md](../../CHANGELOG.md).
- **Contributing**: see [CONTRIBUTING.md](../../CONTRIBUTING.md).
- **Sponsorship**: see the repo's `Sponsor` button or [central sponsor page](https://www.lcv.dev/sponsor).
- **Action pinning**: all GitHub Actions are pinned by full SHA per supply-chain hardening baseline.
- **Code owners**: [.github/CODEOWNERS](../../.github/CODEOWNERS).

## Links

- Site: [https://maestro-app.lcv.dev](https://maestro-app.lcv.dev)
- GitHub: [https://github.com/LCV-Ideas-Software/maestro-app](https://github.com/LCV-Ideas-Software/maestro-app)
- Sponsors: [https://github.com/sponsors/LCV-Ideas-Software](https://github.com/sponsors/LCV-Ideas-Software)

## License

AGPL-3.0-or-later. See [LICENSE](../../LICENSE), [NOTICE](../../NOTICE), and [THIRDPARTY](../../THIRDPARTY.md).

---

<p align="center"><span style="font-size: 1.5em;"><strong>Copyright © 2026 LCV Ideas &amp; Software</strong></span><br><sub>LEONARDO CARDOZO VARGAS TECNOLOGIA DA INFORMACAO LTDA<br>Rua Pais Leme, 215 Conj 1713 - Pinheiros<br>São Paulo - SP - CEP 05424-150<br>CNPJ: 66.584.678/0001-77 - IM: 3039854</sub></p>
