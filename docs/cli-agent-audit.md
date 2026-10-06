# CLI Agent Audit

Status: historical implementation plan with a current permission correction.
Historical snapshot date: 2026-05-23.
Permission contract reviewed: 2026-10-05; AGY transport cleanup: 2026-10-06.

This audit records what Maestro must rely on, verify, and defend against when orchestrating Codex CLI, Claude CLI, and Gemini via Google Antigravity CLI (`agy`) in background.

It is not enough for a CLI to "answer a prompt". Maestro needs predictable non-interactive execution, auth probing, structured output, exit-code handling, stderr capture, tool/permission controls, model provenance, and safe update behavior.

Apart from the current native permission correction below, the suitability findings and proposed structured records remain the dated implementation plan. They do not attest that every proposed field or gate is implemented, or that a CLI has acknowledged reading the complete protocol.

## Historical Local Snapshot

Observed on this Windows 11+ development machine on 2026-05-23. These versions and smoke commands record that historical investigation; they are not the current argument or permission contract.

| Agent | Local command | Local version | Headless smoke | Structured output |
| --- | --- | --- | --- | --- |
| Codex | `codex` | `0.125.0` | `codex --ask-for-approval never exec --skip-git-repo-check --sandbox read-only --color never "<short prompt>"` accepted stdin appended as a `<stdin>` block and returned the requested marker; stdin-only `-` mode was observed hanging in one local probe | Text/JSONL-capable events depending on flags |
| Claude | `claude` | `2.1.119` | `claude --print --input-format text --output-format text --permission-mode dontAsk` accepted stdin and returned the requested marker | Text, JSON object, or stream JSON depending on flags |
| Gemini | `agy` | `1.0.1` | `agy --print "<prompt>" --print-timeout 30s --dangerously-skip-permissions` authenticated through Antigravity and generated output in terminal/print mode, but plain stdout pipes did not capture the answer locally; Maestro therefore runs `agy` through the PTY transport path. | Terminal print stream captured through PTY |

Auth was present for the local smoke tests, but Maestro must never assume the operator's machine is already authenticated.

## Current Native Permission Boundary

Claude Code `dontAsk` denies tool calls that would require approval; it still allows pre-approved tools from inherited permission settings. Maestro therefore limits editorial built-in tools with `--tools Read,Glob,Grep,WebSearch,WebFetch`, denies inherited MCP tools with `--disallowedTools mcp__*`, and uses `--strict-mcp-config` without an MCP configuration to omit ambient MCP servers. Local `Read` remains available for complete oversized prompt sidecars. The smoke probe uses `--tools ""` and the same MCP restrictions. The fixed `--permission-mode dontAsk` option follows these variadic tool options, keeping the smoke's final positional prompt outside their value lists. These are native model-tool restrictions, not an operating-system sandbox or a claim that ambient startup hooks cannot run. See the [official Claude CLI reference](https://code.claude.com/docs/en/cli-reference) and [permission modes](https://code.claude.com/docs/en/permission-modes#allow-only-pre-approved-tools-with-dontask-mode).

Installed Claude Code 2.1.289 accepted both final option sets, including `--strict-mcp-config`, in help-only invocations on 2026-10-05. Those checks did not invoke a provider or prove live tool denial. `--bare` changes authentication by skipping OAuth/keychain discovery and is not used. Exclusive managed MCP configuration can refuse `--strict-mcp-config` at startup; that incompatibility fails closed instead of silently weakening the restriction or changing the operator's selected transport.

Antigravity CLI `--dangerously-skip-permissions` approves tool requests, including file writes and shell commands. A stdout-only instruction in an editorial prompt does not enforce a tool boundary. Installed `agy` 1.2.17 exposes `--mode plan`, but the official contract describes it as a `/plan` instruction prefix; shell permissions still apply across execution modes. `--sandbox` also keeps the workspace writable. Neither option proves read-only editorial execution. The available native permission rules remain configured by Antigravity settings; the installed help does not expose a per-invocation tool whitelist or settings override. See [headless permissions](https://www.antigravity.google/docs/cli/headless/), [execution modes](https://www.antigravity.google/docs/cli/modes/), and [native permissions](https://www.antigravity.google/docs/permissions?tab=cli).

Maestro removes the unconditional Antigravity permission bypass from both editorial and smoke arguments. Workspace writes can still be allowed by default, and inherited settings may authorize further actions. This correction alone cannot justify a read-only claim. Gemini's native API transport does not expose these CLI tools. Retain this distinction when selecting a transport or assessing editorial artifact custody; do not silently alter global CLI settings or introduce an unsupported flag.

Antigravity supports isolated native projects: `agy --new-project` creates one and `agy --project=<project_id>` selects the existing provider-generated identity. The native `/permissions` manager exposes a Project scope where deny rules can be validated and saved before submitting a model prompt. A read-only editorial policy needs deny rules for `write_file(*)`, `command(*)`, `mcp(*)`, and `execute_url(*)`; native Deny rules precede Ask and Allow rules. CLI 1.1.12 also introduced headless `/permissions` readback without starting a model turn. See [native project selection](https://www.antigravity.google/docs/projects/), the [Project permission manager](https://www.antigravity.google/docs/cli/commands/permissions/), and the [official CLI changelog](https://www.antigravity.google/docs/changelog?tab=cli). This documents an available provisioning path; Maestro does not itself provision native policy. Do not infer its enforcement from an unverified project name, a global preset, a TUI list of saved rules, or a successful model response. The selected project's headless permission readback must confirm the policy before editorial use.

Maestro stores the optional AGY CLI project ID locally as `agy_cli_project_id` and passes it through native `--project=<project_id>`. The native configuration reader preserves the same AGY selection saved by source 0.5.71 under its previous field name; new writes use the canonical AGY name. Before each editorial or smoke model turn, it reads the selected project's native `/permissions` JSON in the same working directory. The readback must report success, zero model turns/usage and exactly one Project scope containing all four exact deny rules. Missing selection, malformed output or unconfirmed policy blocks the turn with native project/API guidance; inherited Shared/Global rules do not replace this Project check. API remains an explicit operator choice. The 2026-10-05 native TUI/headless probe did not confirm the policy, so no protected installed AGY CLI profile is claimed.

When the AGY executable is missing, installation opens the [official Windows CLI instructions](https://www.antigravity.google/docs/cli/install/#windows). That documentation page is distinct from the Unix installer returned by the bare `/cli/` endpoint. Setup uses executable/version readiness and does not invent an authentication dependency or offer an AGY login action. Maestro does not open an interactive terminal or launch a browser authentication flow for AGY. The headless editorial invocation reuses the vendor's existing native session and reports actual authentication failures; version metadata and installation instructions do not prove authentication. The separate native Project permission admission check remains unchanged.

## Official Documentation Consulted

- Codex CLI overview and upgrade: https://developers.openai.com/codex/cli
- Codex non-interactive mode: https://developers.openai.com/codex/noninteractive
- Codex authentication: https://developers.openai.com/codex/auth
- Codex approvals and security: https://developers.openai.com/codex/agent-approvals-security
- Claude Code CLI reference: https://code.claude.com/docs/en/cli-reference
- Claude Code environment variables: https://code.claude.com/docs/en/env-vars
- Claude Code permission modes: https://code.claude.com/docs/en/permission-modes
- Google Developers Blog transition notice: https://developers.googleblog.com/en/an-important-update-transitioning-gemini-cli-to-antigravity-cli/
- Antigravity CLI overview: https://antigravity.google/docs/cli-overview
- Antigravity CLI usage/settings: https://antigravity.google/docs/cli-using
- Antigravity CLI features and slash commands: https://antigravity.google/docs/cli-features
- Antigravity CLI getting started: https://antigravity.google/docs/cli-getting-started

## Suitability Findings

### Codex CLI

Codex is suitable for Maestro orchestration through `codex exec`.

Required adapter choices:

- Use `codex exec` for background work, not the interactive TUI.
- Prefer `--json` so Maestro can parse JSONL events.
- Use `--output-schema` when Maestro needs a strict final status block.
- Use `--ephemeral` unless Maestro intentionally wants Codex session files.
- Set explicit sandbox and approval flags per operation.
- Capture stdout and stderr separately. Stderr can contain plugin/cache/sync warnings and provider HTML challenge content even when the actual agent result succeeds.
- Treat Git repository requirements as a preflight condition or use `--skip-git-repo-check` only in controlled Maestro runtime folders.
- Prefer API-key auth for automation when the operator selects API mode; subscription auth remains a separate CLI transport.

### Claude CLI

Claude is suitable for Maestro orchestration through `claude -p`.

Required adapter choices:

- Use `-p` / `--print` for non-interactive runs.
- Prefer `--output-format json` or `--output-format stream-json`.
- Use `--json-schema` for strict output validation where possible.
- Use `--no-session-persistence` for stateless peer rounds unless explicit resume is needed.
- Use `--bare` for controlled scripted calls when Maestro wants to avoid auto-discovery of hooks, memories, keychain reads, plugin sync, and other ambient context.
- Set `--permission-mode` and tool allow/deny lists deliberately.
- Parse API cost and model usage from JSON output for diagnostics and operator budgeting.
- Treat `ANTHROPIC_API_KEY` as overriding subscription auth in non-interactive mode when present.

### Gemini / Antigravity CLI

Gemini remains a Maestro peer identity, and its local CLI transport is Antigravity CLI (`agy`). Maestro neither invokes nor inventories the retired executable.

Required adapter choices:

- Use `agy --print <prompt>` with an explicit `--print-timeout`.
- Keep the internal peer key as `gemini`; only the command transport changes to `agy`.
- Capture `agy` through the PTY runner, because local plain-pipe tests exited successfully without returning the generated answer on stdout.
- Do not rely on `--dangerously-skip-permissions` or prompt-level file prohibitions for editorial custody. Follow the current native permission boundary above; `agy` execution remains subject to its actual permission settings.
- Treat Antigravity tool/web evidence as model-mediated evidence, not raw mechanical verification. Maestro's own Web Evidence Engine remains authoritative for link validation.
- Probe `agy --version` through the bounded managed-pipe metadata transport during dependency preflight and log the actual `agy` resolution. Version metadata confirms the executable; authentication and Project policy require their own native checks.

## Cross-Agent Adapter Contract

`v0.3.1` has the current Tauri-native process adapter pass for real background sessions. It writes full per-agent artifacts under `data/sessions/<run>/agent-runs/` and logs sanitized lifecycle summaries as `session.agent.started` / `session.agent.finished`. On Windows release builds, child processes must be created without visible terminal windows. Real editorial calls have no artificial timeout; only diagnostics may use short bounded probes. The adapter is still a defensive text parser, not the final structured-output implementation described below.

Every CLI adapter must produce this internal record before a peer response can enter convergence:

```json
{
  "agent": "codex | claude | gemini",
  "transport": "cli",
  "cli_path": "absolute path",
  "cli_version": "observed version",
  "auth_status": "ready | auth_required | unknown | failed",
  "model_pin": "requested model id or alias",
  "command": "redacted command vector",
  "stdin_sha256": "hash of prompt bundle",
  "stdout_path": "ignored local artifact path",
  "stderr_path": "ignored local artifact path",
  "exit_code": 0,
  "parsed_status": "READY | NOT_READY | NEEDS_EVIDENCE | status_missing",
  "parse_warnings": [],
  "usage": {},
  "cost": {},
  "duration_ms": 0
}
```

Large prompt handling:

- Inline stdin is allowed only while the prompt bundle stays below Maestro's safe inline threshold.
- For oversized review or revision bundles, Maestro writes the full bundle to an ignored `*-input.md` sidecar file inside `data/sessions/<run>/agent-runs/` and sends the CLI a compact instruction to read that file completely before answering.
- The sidecar file is a runtime artifact, never a Git artifact.
- Agent artifacts record compact stdin size, original prompt size, and the sidecar path so support logs remain understandable without copying a very large prompt into the UI.
- If a CLI exits without output because of a transient pipe, auth, or provider failure, Maestro logs the operational failure and retries through the next review/revision cycle; it must not publish a final text without unanimous READY.

Raw stdout, stderr, prompts, and transcripts are ignored runtime artifacts. The UI receives only sanitized summaries.

## Hard Gates Before Real Adapter Implementation

- Re-run official documentation checks before coding each adapter, because all three CLIs evolve quickly.
- Add golden parser fixtures for successful JSON, JSONL streams, stderr warnings, terminal warning suffixes, malformed JSON, missing status blocks, rate limits, auth failures, and interrupted runs.
- Implement per-agent auth probes instead of inferring auth from command existence.
- Implement per-agent update probes and operator-approved update flows.
- Keep model identity explicit. If a CLI reports only an alias, Maestro must record the alias and mark the exact model as inferred unless independently attested.
- Never let CLI web fetch/search evidence substitute for Maestro's mechanical link checker.

## Open Risks

- CLI output formats are not perfectly symmetrical.
- Plugin sync, telemetry, or terminal warnings may appear outside the final answer stream.
- Auth state can depend on cached user login, API keys, environment variables, or provider-specific files.
- CLI updates can change flags, output fields, and default models.
- Cost/quota behavior differs sharply between subscription auth and API-key auth.

The adapters are therefore feasible, but they must be defensive process adapters rather than thin shell wrappers.
