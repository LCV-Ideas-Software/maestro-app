# Maestro-app — Auditoria de segurança e corretude (2026-06-12) — rev.3 VALIDADA (consolidada)

Cross-review 39f99de1: **rodada 2 unânime 5/5 READY** (codex gpt-5.5, gemini-3.1-pro, deepseek-v4-pro, grok-4.3, perplexity sonar-reasoning-pro). Rodada 1 = 4/5 com 4 pedidos de calibração do Codex, todos atendidos com verificação empírica (S4/S5 rebaixados, causalidade B1 refutada por inspeção de artifacts, R1 promovido a P1).

App desktop Tauri (React 19 + TipTap 3; Rust em `src-tauri/src`, 38 arq/~18.4k linhas). Insumos: código + runtime real (~70 logs NDJSON/8.846 linhas, 7 sessions, `config/ai-providers.json`). Versão em evidência: `package.json:3` = 0.5.33.

Rev.3 consolida a auditoria paralela (cross-review 660dc3d7, também unânime 5/5): acrescenta **F4** (drift de versão TipTap) e **B4** (injeção no human-log, distinto do FP2). Ambas as auditorias confirmam o **FP1** — blocking-em-async não é bug: os runners rodam via `spawn_blocking` + `block_on` (`editorial_agent_runners.rs:175-181`), em thread de bloqueio dedicada.

## Achados (validados)

### Segurança
- **S1 (ALTA)** — SSRF cego no `audit_links`: cliente segue 5 redirects sem revalidar IP por hop (`link_audit.rs:47-49`, `:239-258`); blocklist só roda para host IP-literal — hostname DNS que resolve para IP privado não é checado (`:180-185`). Risco: varredura LAN. Mitigação existente: cego (só status/tone à UI), desktop. **Fix:** `Policy::custom` revalidando IP por hop + resolver DNS do host inicial; ou `Policy::none()`.
- **S2 (MÉDIA)** — `checked_data_child_path` sem canonicalização: junction/symlink dentro de `data/` escapa o gate (`app_paths.rs:151-168`); `..`/absoluto-fora já bloqueados (`:172-177`). **Fix:** `fs::canonicalize` + `starts_with`.
- **S3 (MÉDIA)** — Sanitização inconsistente: AI transform/freeform e gemini-import inserem conteúdo remoto sem DOMPurify (`PostEditor.tsx:231-232`, `:268-269`, `:912-913`); Word/Markdown sanitizam (`:392-394`, `markdownImport.ts:79-81`). Schema do ProseMirror + CSP mitigam no app; risco residual = publicação no mainsite. **Fix:** DOMPurify nas 4 entradas.
- **S4 (BAIXA, hardening)** — Sem allowlist explícita de esquema de URL (`extensions.ts:664-668`); `sanitizeLinksTargetBlank` não filtra esquema (`PostEditor.tsx:716-734`). Default do TipTap v3 bloqueia `javascript:` (implícito). **Fix:** `protocols` explícito + filtro de esquema; follow-up Codex: testar a allowlist para não depender do default do TipTap.
- **S5 (BAIXA, latente)** — `console.warn/error` capturam args crus para NDJSON (`diagnostics.ts:194-211`). Empírico: 0 ocorrências em 8.846 linhas de log. **Fix barato:** allowlist de categorias/redação de strings tipo-HTML.
- **S6 (INFO, by-design)** — 6 chaves de API vivas em texto plano em `data/config/ai-providers.json` (modo `local_json`), em diretório sincronizado pelo OneDrive. **Recomendação:** documentar risco; preferir `windows_env`/secret-store; garantir exclusão de bundle/commit. (Operacional: considerar rotacionar as chaves expostas e mover para `MAESTRO_*` env.)

### Corretude backend
- **B1 (MÉDIA)** — Body cru do provider gravado no artifact em 200-com-shape-inesperado (`provider_runners.rs:684-687`, `:911-914`, `:1139-1141`); DeepSeek omite (correto). Causalidade com violações de contrato foi REFUTADA empiricamente. **Fix:** alinhar com DeepSeek.
- **B2 (MÉDIA)** — Lookup de env no registro Windows case-sensitive (`provider_routing.rs:164-168`): credencial descartada silenciosamente se o case difere → `API_KEY_NOT_AVAILABLE`. **Fix:** comparação case-insensitive; follow-up Codex: parsear o nome exato do valor (evitar colisão de prefixo).
- **B3 (MÉDIA)** — Escrita multi-statement no D1 não-atômica (`cloudflare.rs:903-973`). **Fix:** batch do D1 numa requisição.
- **B4 (BAIXA)** — Injeção de linha no *human-log*: `sanitize_text` não filtra controle/`\n` (`sanitize.rs:49-52`) e a projeção legível grava o resumo cru (`human_logs.rs:101-106`) → `write_log_event` com `\n` no `message` forja linhas no `.log`. Distinto do FP2 (o sink NDJSON é seguro; este é o sink `.log`). **Fix:** stripar/escapar controle/`\n` no `sanitize_text` ou no writer do human-log.

### Frontend
- **F1 (BAIXA-MÉDIA)** — Listener `blur` do FloatingMenu nunca removido (`FloatingMenu.tsx:101-110`). **Fix:** handler nomeado + `editor.off`.
- **F2 (BAIXA)** — `saveFeedbackTimer` sem cleanup no unmount (`PostEditor.tsx:138`, `:691-694`).
- **F3 (BAIXA)** — `globalSearchState` singleton de módulo (`searchReplaceCore.ts:19-22`). **Fix:** PluginState via `tr.setMeta`.
- **F4 (MÉDIA)** — Drift de versão TipTap: `package.json` fixa `@tiptap/{core,react,extension-link,starter-kit}` em 3.26.1, mas `node_modules` tem **3.24.0** em todas. O frontend auditado roda 3.24.0 — inclui o `isAllowedUri` que embasa S3/S4. **Fix:** `npm ci` para reconciliar; re-rodar testes; revalidar o default do TipTap na 3.26.1 antes de confiar nele como backstop de esquema.

### Runtime (logs/sessions)
- **R1 (ALTO impacto)** — Violações de output-contract → não-convergência cara. Prova: run-2026-06-04, round 001 com 13 attempts, `serial_turns:13 > cap:12`, `stable_serial_approvals:0`, **US$ 9,46**, `consensus_ready:false`. Mecanismo confirmado por artifact: Gemini declara `MAESTRO_STATUS: READY` mas omite `maestro_revision_report` → retry pago (~US$0,5-0,9/attempt; ~120-170k input tokens re-enviados). **Fixes:** parser tolerante a desvios menores; violação repetida do mesmo peer → re-prompt com reforço de formato ou pular peer para custódia; cap de retries pagos por round.
- **R2 (MÉDIA)** — Gemini é o peer menos confiável (AGENT_FAILED_NO_OUTPUT 11× numa versão, EMPTY_DRAFT, maior taxa de contract-violation). **Fix:** reforço de contrato específico/tuning de retry.
- **R3 (BAIXA)** — Scheduler serial tentou auto-atribuição 2× (guard `agent_never_reviews_own_current_version` pegou). Revisar lógica de atribuição.
- **R4 (INFO)** — `CODEX_WINDOWS_SANDBOX_UPSTREAM` (falhas do CLI Codex no Windows numa versão).

### Falsos-positivos descartados (validados 5/5)
- **FP1** — "blocking reqwest em async starva tokio": runners rodam via `spawn_blocking` + `block_on` (`session_commands.rs:83`, `editorial_agent_runners.rs:175-181`) — thread de bloqueio dedicada. Não é bug.
- **FP2** — "NDJSON injection via `write_log_event`": serde_json escapa `\n` (`logging.rs:120-168`); `category` via `sanitize_short`. Não é bug **no sink NDJSON** (o sink `.log` legível é coberto por B4).

## Plano de correção (validado)
- **P0:** S1.
- **P1:** R1, S2, S3, B2.
- **P2:** B1, B3, R2, F1.
- **P3:** S4, S5, F2, F3, F4, R3, B4.
- **P4:** S6 (documentação/credenciais), R4 (observar).

Itens P0–P1 com teste que reproduz antes do fix (TDD); `vitest run` + `cargo test` verdes antes/depois. Gate de qualidade de 5 portões antes de qualquer ship.
