<p align="center">
  <img src=".github/assets/lcv-ideas-software-logo.svg" alt="LCV Ideas &amp; Software" width="520" />
</p>

# Maestro Editorial AI

Portable Windows editorial workbench for protocol-driven AI drafting, source verification, and multi-agent editorial convergence.

[![release](https://img.shields.io/github/v/release/LCV-Ideas-Software/maestro-app?sort=semver)](https://github.com/LCV-Ideas-Software/maestro-app/releases)
[![CI](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/ci.yml/badge.svg)](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/ci.yml)
[![Pages](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/pages.yml/badge.svg)](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/pages.yml)
[![Release](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/release.yml/badge.svg)](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/release.yml)
[![CodeQL](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/codeql.yml/badge.svg)](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/codeql.yml)
[![Public Format](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/format-public.yml/badge.svg)](https://github.com/LCV-Ideas-Software/maestro-app/actions/workflows/format-public.yml)
[![status: stable](https://img.shields.io/badge/status-stable-brightgreen.svg)](#status)
[![target: Windows 11+](https://img.shields.io/badge/target-Windows%2011%2B-blue.svg)](#status)
[![stack: Tauri 2 + React 19](https://img.shields.io/badge/stack-Tauri%202%20%2B%20React%2019-blueviolet.svg)](#architecture)
[![license: AGPL-3.0-or-later](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](./LICENSE)

**Status.** Stable. Current release: **v0.5.43** (release tag `v00.05.43`). See [CHANGELOG.md](./CHANGELOG.md) for the full release history.

Operational stable baseline started at `v0.5.25`, with live bootstrap, diagnostics, navigation, Cloudflare credential provisioning, AI API credential checks, PostEditor parity, link auditing, and a real background Claude/Codex/Gemini/DeepSeek/Grok editorial session path. From `v0.5.27`, Maestro also supports Perplexity as an API-only Sonar peer. Runtime evidence from session `run-2026-05-11T01-09-30-556Z` confirms the first documented end-to-end unanimous editorial delivery: Maestro `0.5.25` resumed a real API-mode session, reached `READY_UNANIMOUS`, and wrote a clean `texto-final.md`.

The version history at a glance:

| Release | Scope |
| --- | --- |
| **`v0.5.43`** | Post-merge serial retry hardening: preserves true operator-evidence pauses and scopes corrective retry counters to the exact current draft version. |
| **`v0.5.42`** | Serial corrective retry enforcement: reviewers that detect correctable blockers must correct them on the same turn; internal agent/report communication is `en_US`, while the final operator-facing article remains `pt_BR`. |
| **`v0.5.41`** | Serial reviewer-reviser fix: correctable blockers must be corrected in the same turn, while evidence-required blockers pause for operator evidence instead of circulating indefinitely. |
| **`v0.5.40`** | Security release for Scorecard alert 37: ships the upstream `plist` git pin that resolves `quick-xml` to 0.41.0 and closes RUSTSEC-2026-0194/RUSTSEC-2026-0195 in the distributed bundle. |
| **`v0.5.39`** | Security release for the transitive `anyhow` advisory, closing RUSTSEC-2026-0190 while upstream `plist` work was still pending. |
| **`v0.5.38`** | Circular review closing-turn and final-release audit accounting fixes. |
| **`v0.5.37`** | Final reference audit gate: preserves unresolved evidence markers, rejects bibliographic lacunae in final text, audits public links before `texto-final.md`, and fails closed if final URLs exceed audit capacity. |
| **`v0.5.36`** | Security pin for transitive `undici` advisories and restored repository-hygiene audit gate. |
| **`v0.5.35`** | Security pin for transitive `markdown-it` ReDoS advisory and restored repository-hygiene audit gate. |
| **`v0.5.34`** | Security and robustness audit fixes across the Rust backend and React frontend. |
| **`v0.5.33`** | Frontend XSS hardening in the editor and slash-command UI. |
| **`v0.5.32`** | CI hardening for the Antigravity CLI (`agy`) PTY runner: keeps the PTY writer alive through timeout/cancel handling, stabilizes the timeout test, and clears Rust/BIOME CI gates. |
| **`v0.5.31`** | Gemini local CLI transport migrated from deprecated Gemini CLI to Google Antigravity CLI (`agy`). |
| **`v0.5.30`** | Patch — 4-gate quality directive compliance (eslint + biome + prettier + cross-review). |
| **`v0.5.29`** | Added a live "Agente em turno" indicator that tracks the backend editorial run events and shows which peer is currently drafting, reviewing, or rewriting. |
| **`v0.5.27`** | Perplexity / Sonar as the sixth editorial peer. |
| **`v0.5.26`** | Audit hardening release. |
| **`v0.5.25`** | First operational stable baseline. |
| **`v0.5.24`** | Circular round semantics and resume-safe custody. |
| **`v0.5.23`** | Serial editorial deliberation and link-audit detail. |
| **`v0.5.22`** | Recoverable reviewer operational-outage handling. |
| **`v0.5.21`** | Resume/cycle provenance and operational-failure isolation. |
| **`v0.5.20`** | Internal peer-language and incremental convergence contract. |
| **`v0.5.19`** | Provider prompt-cache activation. |
| **`v0.5.18`** | Site sponsor card iteration. |
| **`v0.5.17`** | Site visual identity refresh. |
| **`v0.5.16`** | Grok as the fifth editorial peer. |
| **`v0.5.15`** | Tribunal-style editorial hardening. |
| **`v0.5.14`** | Resume attempt cost-scope hotfix. |
| **`v0.5.13`** | Resume cost-ledger scope hotfix. |
| **`v0.5.12`** | DeepSeek artifact and API cost-guard hotfix. |
| **`v0.5.11`** | Independent-review and diagnostics hotfix. |
| **`v0.5.10`** | Artifact correctness hotfix. |
| **`v0.5.9`** | Code split batch — frontend track. Extracted `src/types.ts` + `src/constants.ts` + `src/helpers.tsx` from `src/App.tsx`. |
| **`v0.5.8`** | Code split batch — extracted `src-tauri/src/session_orchestration.rs`. |
| **`v0.5.7`** | Code split batch — extracted `src-tauri/src/session_commands.rs`. |
| **`v0.5.6`** | Code split batch — extracted `src-tauri/src/tauri_commands.rs`. |
| **`v0.5.5`** | Code split batch — extracted `src-tauri/src/cloudflare_commands.rs`. |
| **`v0.5.4`** | Code split batch — extracted `src-tauri/src/editorial_io.rs`. |
| **`v0.5.3`** | Hardening pass: D sweep + items_after_test_module fix + CLI cancel artifact status refinement. |
| **`v0.5.2`** | B22 fix — resume path was carrying initial_agent + cost + minutes forward from saved contract over operator's request. |
| **`v0.5.1`** | B21 fix — resume now honors operator's current React state instead of silently overriding with saved session contract. |
| **`v0.5.0`** | Operator-driven session stop with sub-2-second cancel granularity. |
| **`v0.4.0`** | Architectural refactor closing Gemini audit's `too_many_arguments` × 7 finding. |
| **`v0.3.48`** | Code split batch — extracted `src-tauri/src/app_init.rs`. |
| **`v0.3.47`** | Bundled hardening pass closing the remaining P1 set from Codex's audit (#2 DOCX/paste sanitization + Tauri webview CSP allowlist; #3 Cloudflare Secrets Store mode UI clarification) plus 5 of 7 trivial clippy hygiene fixes from Gemini's audit. |
| **`v0.3.46`** | Hardening pass closing P0 set from Codex's read-only audit on HEAD `2ca92e7` (v0.3.45). |
| **`v0.3.45`** | Two operator-directed changes. |
| **`v0.3.44`** | Code split batch — extracted `src-tauri/src/api_payloads.rs`. |
| **`v0.3.43`** | Code split batch — extracted `src-tauri/src/provider_routing.rs`. |
| **`v0.3.42`** | Behavior fix — B20 backend completion: saved cost/time caps were being silently re-applied on resume even when the operator's form was blank. |
| **`v0.3.41`** | Code split batch — extracted `src-tauri/src/config_persistence.rs`. |
| **`v0.3.40`** | Code split batch — extracted `src-tauri/src/provider_config.rs`. |
| **`v0.3.39`** | Code split batch — extracted `src-tauri/src/cli_adapter.rs`. |
| **`v0.3.38`** | Behavior fix — provider mode toggle (Hibrido/CLI/API) was being ignored for DeepSeek. |
| **`v0.3.37`** | Code split batch — extracted `src-tauri/src/session_minutes.rs`. |
| **`v0.3.36`** | Code split batch — extracted `src-tauri/src/editorial_agent_runners.rs`. |
| **`v0.3.35`** | Code split batch — extracted `src-tauri/src/command_spawn.rs`. |
| **`v0.3.34`** | Code split batch — extracted `src-tauri/src/sanitize.rs`. |
| **`v0.3.33`** | Code split batch — extracted `src-tauri/src/command_path.rs`. |
| **`v0.3.32`** | B20 fix — resume should NOT carry forward saved cost/time caps. |
| **`v0.3.31`** | Code split batch — extracted `src-tauri/src/link_audit.rs`. |
| **`v0.3.30`** | Code split batch — extracted `src-tauri/src/ai_probes.rs`. |
| **`v0.3.29`** | Code split batch — extracted `src-tauri/src/session_artifacts.rs`. |
| **`v0.3.28`** | Code split batch — extracted `src-tauri/src/session_resume.rs`. |
| **`v0.3.27`** | Code split batch — extracted `src-tauri/src/session_persistence.rs`. |
| **`v0.3.26`** | Code split batch — extracted `src-tauri/src/editorial_inputs.rs`. |
| **`v0.3.25`** | Code split batch — extracted `src-tauri/src/editorial_prompts.rs`. |
| **`v0.3.24`** | Code split batch — extracted `src-tauri/src/editorial_helpers.rs`. |
| **`v0.3.23`** | Code split batch — extracted `src-tauri/src/cloudflare.rs`. |
| **`v0.3.22`** | Code split batch — bundled 3 isomorphic provider runners. |
| **`v0.3.21`** | Code split batch — extracted `src-tauri/src/provider_deepseek.rs` (the structural outlier). |
| **`v0.3.20`** | Code split batch — extracted `src-tauri/src/provider_retry.rs`. |
| **`v0.3.19`** | Code split batch — extracted `src-tauri/src/logging.rs`. |
| **`v0.3.18`** | B17/B18/B19 production-bug fixes deferred from v0.3.17 split. |
| **`v0.3.17`** | Code split batch — extracted `src-tauri/src/app_paths.rs`. |
| **`v0.3.16`** | Pos-v0.3.15 production-log fixes + Codex NB-2/NB-5 hardenings. |
| **`v0.3.15`** | Anti-"casca vazia" sweep — 12 distinct fixes from session-log analysis. |
| **`v0.3.14`** | Rigorous security/UX audit closure. |
| **`v0.3.13`** | Session controls, API peers, attachments, and code splitting. |
| **`v0.3.12`** | README organizational standardization. |
| **`v0.3.11`** | DeepSeek real-peer integration. |
| **`v0.3.10`** | Long-run orchestration reliability. |
| **`v0.3.9`** | Cloudflare persistence and settings maturation. |
| **`v0.3.8`** | Draft-lead selection for editorial sessions. |
| **`v0.3.7`** | CodeQL `rust/path-injection` regression-test fix. |
| **`v0.3.6`** | Portable Windows startup path fix. |
| **`v0.3.5`** | Hardened resumable session filesystem scans so session folders and agent artifacts are reconstructed only from validated safe names. |
| **`v0.3.4`** | Reworked the running session status card to use an indeterminate activity meter instead of an artificial completion percentage. |
| **`v0.3.3`** | Canonical app-root validation for native filesystem access. |
| **`v0.3.2`** | Transferred the public repository from `example-beneficiary/maestro-app` to `lcv-ideas-software/maestro-app`. |
| **`v0.3.1`** | Fixed Windows child-process spawning so CLI probes, registry env-var reads, `.cmd` shims, PowerShell wrappers, and editorial agents run with `CREATE_NO_WINDOW` and no visible terminal windows. |
| **`v0.3.0`** | Replaced the UI-only CLI smoke path with a real first-pass editorial session command that runs Claude for draft generation and Claude, Codex, and Gemini for review in background. |
| **`v0.2.1`** | Fixed Windows CLI preflight detection for npm-style `.cmd` shims and known user install paths so Codex, Gemini, npm, and similar CLIs are not incorrectly shown as missing when they are installed. |
| **`v0.2.0`** | Updated the release artifact upload/download actions to Node 24-capable versions to remove GitHub Actions Node 20 deprecation warnings. |
| **`v0.1.0`** | Initial architecture planning for a portable Windows editorial workbench. |

Maestro is independent from `cross-review-mcp`; it incorporates the same strict convergence discipline in its own application logic. It is designed to run from a folder, keep runtime data out of Git, and store operator protocols, drafts, evidence, and sessions locally under ignored runtime paths.

Target platform: Windows 11+.

Planned modern stack: Tauri 2 + WebView2, React 19, Vite 8, TypeScript 6, Vitest, Biome, ESLint, and lucide-react.

Diagnostic logs are structured NDJSON files under `data/logs/`, one file per app execution, with native/frontend context and per-agent process events so failures can be attached for precise analysis. The app UI shows a human-readable activity summary while the raw NDJSON remains available for deep debugging. See `docs/logging.md`.

CLI agents run in background by design, without visible terminal windows in Windows release builds. Claude and Codex use their local CLIs; Gemini uses Google Antigravity CLI (`agy`) as the local CLI transport after Google's Gemini CLI transition. DeepSeek, Grok, and Perplexity run through official API paths, not local CLIs. Real editorial calls do not have an artificial timeout. The operator can choose Claude, Codex, Gemini, DeepSeek, Grok, or Perplexity to write the first version; that choice is saved with the session. Maestro applies a tribunal-style editorial model: the agent that authored the current draft/revision is the petitioner for that cycle and can revise/contest through the next cycle, but cannot vote as reviewer of the same text. Review rounds use only independent active peers; if the author cannot be verified or no independent reviewer remains, the session pauses instead of allowing self-review. The operator sees friendly progress, elapsed-time heartbeat status, phase status, resume controls, and a selectable UI verbosity level, while raw prompts, stdout, stderr, working drafts, and transcripts stay out of the normal interface and remain protected as ignored local runtime artifacts under `data/sessions/`.

Perplexity configuration:

- Key: set `MAESTRO_PERPLEXITY_API_KEY` or `PERPLEXITY_API_KEY`, or enter the key in **Ajustes > Agentes via API**.
- Model override: optional `MAESTRO_PERPLEXITY_MODEL` or `PERPLEXITY_MODEL`; default is `sonar-reasoning-pro`.
- Cost controls: configure both Perplexity input/output USD-per-million-token rates before paid API sessions. Maestro blocks paid API calls without explicit user-provided rate and session-cost ceilings.
- Transport: Perplexity is disabled in **CLI** mode, enabled in **Hybrid** and **API** modes, and uses text/manifest attachment fallback rather than native file uploads.

MainSite-bound editing uses a PostEditor parity module, not a generic editor. See `docs/text-editor-decision.md` and `docs/mainsite-compatibility-contract.md`.

First-run dependency checks, authorized background installation, CLI setup, and authentication flows are planned under `docs/runtime-bootstrapper.md`.

CLI adapter feasibility and risks are audited under `docs/cli-agent-audit.md`.

Cloudflare account/token configuration now verifies the token, prepares `maestro_db`, reuses an existing account Secrets Store when present, and creates `maestro` only when no store exists and creation is permitted. Broader API-first D1 publishing remains tracked under `docs/cloudflare-credentials.md`.

Official AI provider API credentials can be saved locally in `data/config/ai-providers.json` and verified against OpenAI, Anthropic, Gemini, DeepSeek, Grok, and Perplexity model-list endpoints. API-backed editorial sessions require provider tariffs plus an explicit per-session USD cost limit; paid calls do not run with an implicit or hard-coded unlimited budget. Full SDK orchestration remains tracked under `docs/ai-provider-credentials.md`, alongside the existing CLI path.

Configuration persistence supports three modes: local JSON for everything, Windows env-var hybrid for tokens/API keys plus JSON for other settings, and Cloudflare remote persistence through D1 `maestro_db` plus Cloudflare Secrets Store. See `docs/configuration-persistence.md`.

The portable ZIP includes `LEIAME.md` with first-run instructions for end users, including `data/config/bootstrap.json`, Cloudflare environment variables, and per-execution NDJSON logs.

The growing native and React surfaces now have a staged modularization plan in `docs/code-split-plan.md`.

Prompt-to-consensus sessions export separate final text and session minutes. Interrupted sessions can be resumed from `data/sessions/`; if a new protocol is loaded before resume, Maestro passes it to the agents and preserves the previous protocol as a local session artifact. See `docs/editorial-session-workflow.md`.

Shared chat import, Markdown/PDF support, and Cloudflare D1 integration are planned under `docs/import-export-cloudflare.md`.

Web fetch, curl-compatible replay, web search, rendered collection, and human-assisted browser capture are planned under `docs/web-evidence-engine.md`.

ABNT citation/reference formatting and Maestro's deterministic fourth-peer role are planned under `docs/abnt-citation-engine.md`.

Link checking, sanitization, correction proposals, and cross-review escalation are planned under `docs/link-integrity-engine.md`.

## Repository conventions

- **License**: [AGPL-3.0-or-later](./LICENSE). Network-service trigger applies: running a modified fork as a public service obligates you to publish modifications.
- **Notices**: see [NOTICE](./NOTICE) and [THIRDPARTY](./THIRDPARTY.md).
- **Security disclosure**: see [SECURITY.md](./SECURITY.md).
- **Code of conduct**: see [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md).
- **Changelog**: [CHANGELOG.md](./CHANGELOG.md).
- **Contributing**: see [CONTRIBUTING.md](./CONTRIBUTING.md).
- **Sponsorship**: see the repo's `Sponsor` button or [central sponsor page](https://www.lcv.dev/sponsor).
- **Action pinning**: all GitHub Actions are pinned by full SHA per supply-chain hardening baseline.
- **Code owners**: [.github/CODEOWNERS](.github/CODEOWNERS).

## Links

- Site: [https://maestro-app.lcv.dev](https://maestro-app.lcv.dev)
- GitHub: [https://github.com/LCV-Ideas-Software/maestro-app](https://github.com/LCV-Ideas-Software/maestro-app)
- Sponsors: [https://github.com/sponsors/LCV-Ideas-Software](https://github.com/sponsors/LCV-Ideas-Software)

## License

AGPL-3.0-or-later. See [LICENSE](./LICENSE), [NOTICE](./NOTICE), and [THIRDPARTY](./THIRDPARTY.md).

---

<p align="center"><span style="font-size: 1.5em;"><strong>Copyright © 2026 LCV Ideas &amp; Software</strong></span><br><sub>LEONARDO CARDOZO VARGAS TECNOLOGIA DA INFORMACAO LTDA<br>Rua Pais Leme, 215 Conj 1713 - Pinheiros<br>São Paulo - SP - CEP 05424-150<br>CNPJ: 66.584.678/0001-77 - IM: 3039854</sub></p>
