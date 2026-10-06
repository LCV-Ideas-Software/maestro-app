# AI Provider Credentials

Status: direct API editorial contracts, native reasoning and prompt caching, completion validation, and per-session cost controls.
Documentation reviewed: 2026-10-05. Account-specific model access and quota still require authenticated verification.

Maestro must keep CLI orchestration and official API/SDK orchestration as first-class options. The operator should be able to use a subscription-backed CLI when that is convenient, or provide API credentials and pay through provider credits when that is the better path.

## Provider Modes

- `cli`: use the local `codex`, `claude`, and Google Antigravity `agy` CLIs.
- `api`: use official provider APIs/SDKs only.
- `hybrid`: Claude, Codex, and Gemini use their CLIs; DeepSeek, Grok, and Perplexity use their APIs. Routing is determined by peer identity and does not change when an API key is added or removed.

No mode changes the convergence rule. Claude, Codex/OpenAI, Gemini, DeepSeek, Grok, Perplexity, and MaestroPeer still need unanimous, independent `READY` votes for the same accepted version before final delivery when those peers are active in the session.

From `v0.5.27`, the operator can select 1 to 6 active AI peers per session. Unselected peers are not called and do not count toward that session's unanimity gate.

## Settings Fields

The settings screen provides secure credential fields and UI-owned tariff rows for:

- OpenAI / Codex: API key and input/output USD per 1M tokens.
- Anthropic / Claude: API key and input/output USD per 1M tokens.
- Google / Gemini: Gemini Developer API key and input/output USD per 1M tokens.
- DeepSeek: API key and input/output USD per 1M tokens.
- Grok / xAI: API key and input/output USD per 1M tokens.
- Perplexity / Agent API: API key and input/output USD per 1M tokens.

UI model pins and organization/project routing are future configuration fields. Current execution resolves models from authenticated provider model-list endpoints, preferring documented flagship models. Explicit existing model environment overrides take precedence; discovery failure preserves the documented fallback rather than inventing a different peer.

The UI must also keep each provider's CLI path/version/auth status visible because CLI and API operation are independent readiness surfaces.

## Current Editorial API Contracts

These are direct API targets; subscription CLIs select models through their own official configuration.

| Provider | Preferred model | Native endpoint | Maximum configured reasoning |
| --- | --- | --- | --- |
| OpenAI | `gpt-6-astra` | `/v1/responses` | `reasoning.effort: "max"` |
| Anthropic | `claude-fable-5-1` | `/v1/messages` | adaptive thinking, `output_config.effort: "max"` |
| Google | `gemini-3.1-pro-preview` | `models/{model}:generateContent` | `thinkingConfig.thinkingLevel: "high"` |
| DeepSeek | `deepseek-v4-pro` | `/chat/completions` | thinking enabled, `reasoning_effort: "max"` |
| xAI | `grok-4.7` | `/v1/responses` | `reasoning.effort: "xhigh"` |
| Perplexity | `perplexity/kimi-k3` | `/v1/agent` | `reasoning.effort: "high"`, native `web_search` |

The shared draft/review ceiling is 65,536 output tokens, including reasoning where the provider counts it in that limit. OpenAI's legacy `gpt-4.1` fallback is capped at its native 32,768 output tokens. This is headroom, not an instruction to fill the available tokens. Anthropic's documented SDK streaming guard above 21,333 tokens is a client guard; it is not an API restriction on this direct REST adapter. Editorial calls have no additional fixed deadline and remain cancellable. Model discovery uses a separate 30-second metadata timeout.

Only completed visible assistant text can create a draft or a `READY` vote. Partial, truncated, filtered, failed, tool-only, thinking-only, and malformed responses produce a failure artifact. Gemini uses the first completed candidate and excludes thought parts. A paid response rejected by these checks still contributes its reported usage and available cost to the session ledger.

The current adapters issue independent editorial turns. They do not run a client-side function/tool continuation loop or silently turn a partial result into a successful revision. Perplexity's server-side native web search remains subject to the existing typed evidence and completed-response checks.

## Validation

When a credential is entered, Maestro must validate in layers:

1. Local syntax check and redaction test.
2. Provider reachability.
3. Authentication check against the official API.
4. Model availability check for the configured model pin.
5. Rate-limit, quota, or billing-readiness check when the provider exposes it.
6. A minimal non-destructive smoke request after explicit operator approval.

Validation statuses:

- `not_configured`
- `redaction_failed`
- `auth_failed`
- `model_unavailable`
- `quota_unavailable`
- `rate_limited`
- `ready`

Implemented through v0.3.13:

- The settings screen has explicit `Salvar APIs` and `Verificar APIs` actions.
- Local JSON persistence writes `data/config/ai-providers.json`, which remains under ignored runtime data.
- Verification calls official model-list endpoints for OpenAI, Anthropic, Gemini, DeepSeek, Grok, and Perplexity and reports provider-level status without logging raw keys.
- Perplexity's model catalog is public: a successful catalog response confirms availability, while credential validation requires an authenticated Agent call. A rate-limit response is reported as inconclusive credential validation.
- Network-error rendering strips request URLs before messages reach the UI/logs, so query-string API keys are not echoed when a provider request fails before a response is received.
- DeepSeek, OpenAI/Codex, Anthropic/Claude, and Google/Gemini can generate drafts, review drafts, and produce revisions through direct provider APIs. At runtime, Maestro asks each authenticated model-list endpoint which models are available and selects the strongest supported entry for that provider. DeepSeek still honors `MAESTRO_DEEPSEEK_MODEL` or `CROSS_REVIEW_DEEPSEEK_MODEL` when set.
- Grok/xAI and Perplexity Agent API run API-only in API and hybrid modes. CLI mode disables them instead of pretending local CLI transports exist.
- An explicit session-level USD ceiling is required whenever any selected peer uses an API. Accounting uses reported usage and pre-call estimates. Input token projections use a character heuristic, so they are not a guaranteed provider bill or a monetary reservation. The limit remains one session-level value; Maestro never creates per-model budgets or silently drops a selected peer to stay under budget.
- Provider tariffs are UI-owned configuration. The operator maintains input/output USD per 1M tokens in `Configuracoes > Agentes via API > Tabela de tarifas`; there is no env-var fallback for cost rates. Any peer that will run through a direct provider API is blocked with a friendly message until both tariff fields for that provider are configured.
- CLI-backed peers expose no reliable per-call token usage to Maestro yet. Their cost is displayed as unknown/subscription and does not decrement the session USD ceiling required by any API-backed peers. A CLI-only roster does not require that ceiling.
- Windows env-var read is active for provider keys; write UX is still pending.
- Cloudflare Secrets Store persistence writes provider keys and reloads secret references, but raw values remain non-readable from the desktop app by design.

Still pending:

- Windows env-var write UX for provider keys.
- Cloudflare-side broker or AI Gateway integration for consuming Secrets Store values without exposing them back to the desktop app.
- Optional UI model pinning per provider. Current direct API execution resolves models dynamically from each provider's model-list endpoint, with conservative fallbacks.
- Cloudflare-side broker or AI Gateway cost telemetry for hosted/remote execution paths.

## Prompt Cache Policy

Prompt caching is used only when it can reduce paid input cost without weakening the editorial protocol, disabling thinking, or changing the selected model. Cache configuration never stores raw prompts, API keys, or protocol text in public files.

Implemented through `v0.5.19`:

- OpenAI/Codex direct Responses calls send a deterministic `prompt_cache_key`. GPT-5.6 and GPT-6 models use the current native `prompt_cache_options: { "ttl": "30m" }` contract. Supported older models use `prompt_cache_retention: "24h"`; unknown models keep the key and omit a retention override. Standard service tier is requested explicitly. Cache-write tokens are included in total input tokens and billed at 1.25 times the configured input tariff; cache reads use the documented model multiplier. Long-context surcharges are included in estimated cost for current models.
- Anthropic/Claude direct Messages calls send the stable `system` prompt as a text block marked with `cache_control: { "type": "ephemeral" }`, which enables prompt caching with the provider's default short retention when the stable prefix is long enough. Total input usage includes uncached, cache-creation, and cache-read tokens. Five-minute cache writes are estimated at 1.25 times the configured input tariff; cache reads use the ordinary input tariff as a conservative estimate because settings do not yet expose a separate cache-read tariff.
- DeepSeek uses the provider's automatic disk cache. Maestro does not add non-standard request fields; it records `prompt_cache_hit_tokens` and `prompt_cache_miss_tokens` when DeepSeek returns them.
- Grok/xAI direct Responses calls send a deterministic `prompt_cache_key` and parse cached-token usage fields when present. Native `usage.cost_in_usd_ticks` is converted to observed USD by dividing by 10,000,000,000, including provider discounts and server-side tool costs; configured tariffs remain the fallback estimate.
- The Perplexity Agent API currently has no documented prompt-cache control comparable to the other direct editorial flows, so Maestro does not add invented cache fields. It logs the provider cache plan as provider-automatic/unsupported metadata only.
- Gemini keeps the GenerateContent payload thinking-preserving. Explicit Gemini cached-content resources are not forced from the desktop runner because the current quality requirement is to preserve thinking mode; Maestro records provider cache usage if `usageMetadata.cachedContentTokenCount` is returned.
- Each API peer writes non-secret cache policy metadata to NDJSON and to `data/sessions/<run>/cache-manifest.ndjson`: provider, model, role, cache mode, cache key hash, retention label, stable-prefix character count, and prompt character count.
- Each successful API artifact includes cache mode, key hash, control status, retention, cached input tokens, hit tokens, miss tokens, read tokens, and creation tokens where known.

The cache key hash is derived from provider, model, role, agent name, and the stable system prompt. It is meant for diagnostics and provider routing only; it is not a secret and it is not enough to reconstruct the prompt.

If a provider credential is invalid or underfunded, Maestro must explain which provider path is blocked and whether the CLI path can still satisfy that peer.

## Security

- Never store raw provider API keys in Git-tracked files.
- Never include provider keys in cross-review prompts, session minutes, Markdown exports, support bundles, or raw UI activity.
- Store keys only in the ignored local encrypted vault once implemented.
- During early development, any fallback local config must remain ignored and clearly marked unsafe for production.
- Logs may include provider name, key presence, validation status, request IDs, and redacted fingerprints only.
- Do not print provider keys into shell commands or process arguments when an SDK or stdin/config-file handoff can avoid it.

## Storage Options

The operator chooses one of the three persistence modes defined in `docs/configuration-persistence.md`:

- Local JSON: all provider credentials and configuration are stored in ignored JSON files.
- Windows environment variables: provider API keys are stored in user-scope env vars; model pins, route preferences, and non-secret settings remain in JSON.
- Cloudflare: provider profile settings are stored in D1 `maestro_db`; raw API keys are written to Cloudflare Secrets Store and D1 stores only secret references.

Cloudflare mode caveat:

- Secrets Store values are not read back in plaintext. Local desktop adapters that need raw provider keys must either receive a fresh operator-provided value for that session or route through a Cloudflare-side broker that can consume the secret without exposing it to Maestro.

Suggested user-scope Windows environment variable names:

- `MAESTRO_OPENAI_API_KEY`
- `MAESTRO_OPENAI_ORG_ID`
- `MAESTRO_OPENAI_PROJECT_ID`
- `MAESTRO_ANTHROPIC_API_KEY`
- `MAESTRO_GEMINI_API_KEY`
- `MAESTRO_DEEPSEEK_API_KEY`
- `MAESTRO_GROK_API_KEY`
- `MAESTRO_PERPLEXITY_API_KEY`
- `MAESTRO_GOOGLE_VERTEX_PROJECT`
- `MAESTRO_GOOGLE_VERTEX_LOCATION`

Machine-wide environment variables require administrator elevation, a command preview, and a post-action verification step. Current-user variables are preferred.

## Official API Notes

Implementation must re-check current provider documentation before coding each adapter because APIs, SDKs, model names, and authentication rules change often.

Current planning references:

- OpenAI API keys use bearer authentication and may include organization/project headers for multi-project accounts.
- OpenAI direct calls use the Responses API at `/v1/responses`, bearer auth, `input`, `instructions`, `max_output_tokens`, and response `usage.input_tokens`/`usage.output_tokens` for cost accounting.
- Anthropic direct calls use Messages API at `/v1/messages` with `x-api-key`, `anthropic-version`, `model`, `max_tokens`, `system`, and `messages`; response `usage.input_tokens`/`usage.output_tokens` feeds the cost ledger.
- Gemini direct calls use `models/{model}:generateContent` with API-key auth in the `x-goog-api-key` header, `contents`, `systemInstruction`, and `generationConfig.maxOutputTokens`. Billable output usage includes both `candidatesTokenCount` and `thoughtsTokenCount`; API keys are absent from request URLs.
- Grok/xAI direct calls use the OpenAI-compatible Responses API at `https://api.x.ai/v1/responses`, bearer auth, `input`, `max_output_tokens`, `store: false`, and `prompt_cache_key`.
- Perplexity direct calls use the Agent API at `POST https://api.perplexity.ai/v1/agent`, bearer auth, a `provider/model` ID, `input`, `instructions`, `max_output_tokens`, `reasoning.effort`, and an explicit `web_search` tool. Only `completed` assistant `message` items with `output_text` content become editorial text; typed search results and annotation URLs are logged as source metadata. The provider-reported `usage.cost.total_cost` includes tool fees when available.
- Current Perplexity documentation advertises Kimi K3 maximum reasoning through `max` or `xhigh`, but authenticated native probes on 2026-10-05 rejected both aliases with HTTP 400, including the official Responses alias and a `medium` preset with explicit Kimi. `high` is the highest verified accepted level: the full editorial request with 65,536 output headroom completed and returned typed web-search evidence plus billable reasoning tokens. Maestro uses this explicit working contract and does not silently retry with a lower effort or substitute another model.
- Direct API attachments are provider-shaped: OpenAI receives supported images as `input_image` and supported documents as `input_file` with base64 data URLs; Anthropic receives supported images and PDFs as base64 content blocks; Gemini receives supported media/documents as `inline_data` parts; Grok receives PNG/JPEG images as Responses `input_image` data URLs, including normalization of `image/jpg` to `image/jpeg`.
- The selected DeepSeek V4 Pro contract has text-only input. It receives the existing bounded text previews, while binary files without previews remain metadata only. Perplexity's current adapter likewise uses manifest metadata and bounded text previews. These adapters explicitly tell the peer what was delivered and that local paths and metadata do not establish access to file contents; they preserve useful previews instead of rejecting a legitimate text-only request.
- Attachment types that are not natively supported by the selected provider, or that exceed the native API inline size cap, remain available through the session manifest and bounded text previews. Native attachment payload size is included in the conservative pre-call cost projection.
- Before sending native attachments, Maestro measures the complete serialized JSON request, including base64 and envelope bytes. Current limits are 512 MB for OpenAI (with a separate 50 MB combined decoded document limit), 32 MB for Anthropic, 32 MiB for Perplexity, 48 MiB for DeepSeek, and 100 MB for Gemini. Gemini audio/video inline requests use the stricter 20 MB total request ceiling in the modality guides. Grok's documented 20 MiB limit applies to decoded bytes per image; its base64/envelope may exceed 20 MiB. No undocumented aggregate Grok request ceiling is inferred from another provider or the separate Batch API. The existing 20 MiB per-file inline limit remains a conservative local limit for other adapters. Gemini uses its documented image/audio/video MIME lists, including HEIC/HEIF; Office/ODF, GIF, and unknown media MIME blobs are kept in previews instead of being sent as unsupported native parts. Other providers retain their documented formats.
- The session UI mirrors this as a pre-run per-provider prediction, so mixed support is visible before invocation instead of collapsed into a single native/manifest label.
- DeepSeek supports OpenAI-compatible bearer authentication at `https://api.deepseek.com`; the direct peer uses `/models` for verification/model selection and `/chat/completions` for editorial calls. The preferred reasoning model remains `deepseek-v4-pro`; `deepseek-flash` is the current documented flash alias. Retired `deepseek-v4-flash` is retained only as an availability fallback for catalogs that still expose it. Legacy `deepseek-chat` and `deepseek-reasoner` are excluded from the resolver.

Official documentation:

- OpenAI API authentication: https://platform.openai.com/docs/api-reference/authentication
- OpenAI Responses API: https://platform.openai.com/docs/api-reference/responses/create
- Anthropic API authentication: https://docs.anthropic.com/en/api/getting-started
- Anthropic Messages API: https://docs.anthropic.com/en/api/messages
- Gemini API keys: https://ai.google.dev/tutorials/setup
- Gemini GenerateContent API: https://ai.google.dev/api/generate-content
- Google Gen AI SDK / Gemini Developer API and Vertex AI: https://ai.google.dev/gemini-api/docs/migrate-to-cloud
- DeepSeek API quick start: https://api-docs.deepseek.com/
- DeepSeek model list endpoint: https://api-docs.deepseek.com/api/list-models
- DeepSeek native request-body limit: https://api-docs.deepseek.com/guides/vision/
- xAI native image inputs and decoded size/MIME limits: https://docs.x.ai/developers/model-capabilities/images/understanding
- xAI Responses API / prompt caching: https://docs.x.ai/docs/guides/prompt-caching
- Perplexity Agent API: https://docs.perplexity.ai/api-reference/agent-post
- Perplexity Agent API models: https://docs.perplexity.ai/docs/agent-api/models
- Perplexity model list endpoint: https://docs.perplexity.ai/api-reference/models-get
- OpenAI GPT-6 Astra capabilities: https://developers.openai.com/api/docs/models/gpt-6-astra
- OpenAI current prompt caching: https://developers.openai.com/api/docs/guides/prompt-caching
- Claude Fable 5.1 contract: https://platform.claude.com/docs/en/models/fable-5-1/whats-new-fable-5-1
- Claude thinking and streaming guidance: https://platform.claude.com/docs/en/about-claude/models/extended-thinking-models
- Gemini 3.1 Pro model: https://ai.google.dev/gemini-api/docs/models/gemini-3.1-pro-preview
- Gemini native image/audio/video formats: https://ai.google.dev/gemini-api/docs/image-understanding ; https://ai.google.dev/gemini-api/docs/audio ; https://ai.google.dev/gemini-api/docs/video-understanding
- Perplexity native request limits and field contract: https://github.com/perplexityai/api-platform-developers/blob/main/skills/migrate-sonar-to-agent-api/references/request-mapping.md
- DeepSeek current limits and aliases: https://api-docs.deepseek.com/quick_start/pricing
- xAI Grok 4.7: https://docs.x.ai/developers/models/grok-4.7
- xAI actual billed cost: https://docs.x.ai/developers/cost-tracking

## Cross-Review Use

Provider credential status is part of session readiness. A peer is unavailable if neither its CLI path nor its API path is validated for the configured session.

Maestro must record which path produced each agent response:

```json
{
  "provider": "openai | anthropic | google | deepseek | xai | perplexity",
  "agent": "codex | claude | gemini | deepseek | grok | perplexity",
  "transport": "cli | api_sdk",
  "model_pin": "provider-model-id",
  "credential_ref": "local-vault-reference",
  "request_id": "provider-request-id-or-null"
}
```

`credential_ref` is an opaque local reference and must never be a real key.
