# Contributing to Maestro Editorial AI

Maestro is an operational stable-baseline project. Contributions should preserve the portable Windows/Tauri runtime, the editorial protocol boundaries, and the repository security and release gates.

## Engineering rules

- Treat GitHub Secret Scanning, Code Scanning, CodeQL, Dependabot, dependency review, and release automation as active gates.
- Do not commit private editorial protocols, user drafts, evidence caches, CLI transcripts, credentials, tokens, `.env` files, local app data, or generated runtime artifacts.
- Use sanitized placeholders such as `<api_key_redacted>` for examples and fixtures.
- Keep source changes aligned with the current README, changelog, and public release state.
- Runtime data belongs under ignored `data/` paths, never in tracked fixtures.
- Preserve `main` as the only permanent branch and keep release tags aligned with the padded organization convention.

## Pull requests

- Keep changes focused and small enough to review safely.
- Include validation evidence for security-sensitive, workflow, dependency, provider, or release changes.
- Run the relevant local checks documented in the repository before requesting review.
- Do not bypass or weaken the multi-peer/cross-review discipline documented for this project.

## Security reports

Do not open public issues for suspected vulnerabilities, credential leaks, private editorial material exposure, authentication bypasses, executable-supply-chain issues, or deployment misconfiguration. Use [SECURITY.md](./SECURITY.md) and report privately to `lcv@lcv.dev`.

## Conduct

By participating, you agree to follow [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md) (Contributor Covenant 3.0).
