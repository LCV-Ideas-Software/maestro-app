// Modulo: src/types.ts
// Descricao: Type aliases and interface definitions extracted from
// `src/App.tsx` in v0.5.9 per `docs/code-split-plan.md` (frontend track).
// Pure data-only extraction — every type definition preserved verbatim
// from App.tsx v0.5.8 (commit cbfc02d). No runtime values, no React
// components, no hooks, no behavior. Only the home of these declarations
// moved.

import type { ComponentType } from "react";

export type ProtocolSnapshot = {
  name: string;
  size: number;
  lines: number;
  hash: string;
};

export type AgentState = "ready" | "blocked" | "evidence" | "running";
export type VerbosityMode = "resumo" | "detalhado" | "diagnostico";
export type PhaseState = "done" | "active" | "waiting";
export type ProviderMode = "cli" | "api" | "hybrid";
export type AiCredentialKey =
  | "openai"
  | "anthropic"
  | "gemini"
  | "deepseek"
  | "grok"
  | "perplexity";
export type InitialAgentKey = "claude" | "codex" | "gemini" | "deepseek" | "grok" | "perplexity";
export type ProviderRateKey = AiCredentialKey;
export type NativeAttachmentProvider = Exclude<AiCredentialKey, "deepseek" | "grok" | "perplexity">;
export type CredentialStorageMode = "local_json" | "windows_env" | "cloudflare";
export type CloudflareTokenSource = "prompt_each_launch" | "windows_env" | "local_encrypted";
export type ActiveSection = "session" | "protocols" | "evidence" | "agents" | "settings" | "setup";
export type SettingsTab = "providers" | "cloudflare";
export type RunStatus = "idle" | "preparing" | "running" | "paused" | "blocked" | "completed";
export type ActivityLevel = "summary" | "detail" | "diagnostic";
export type NavItem = {
  section: ActiveSection;
  label: string;
  icon: ComponentType<{ size?: number }>;
};

export type OperationSnapshot = {
  title: string;
  progress: number;
  current: string;
  eta: string;
  status: RunStatus;
};

export type AgentCard = {
  name: string;
  cli: string;
  state: AgentState;
  note: string;
};

export type ActivityItem = {
  level: ActivityLevel;
  time: string;
  title: string;
  detail: string;
};

export type PhaseItem = {
  label: string;
  detail: string;
  state: PhaseState;
};

export type DiscussionRound = {
  round: string;
  status: string;
  note: string;
};

export type EvidenceRow = {
  label: string;
  value: string;
  tone: "idle" | "ok" | "warn" | "danger" | "info";
};

export type CloudflarePermissionRow = {
  label: string;
  value: string;
  tone: "pending" | "blocked" | "ok" | "warn" | "error";
};

export type BootstrapCheckRow = {
  label: string;
  value: string;
  tone: "pending" | "blocked" | "ok" | "warn";
};

export type BootstrapConfig = {
  schema_version: number;
  credential_storage_mode: CredentialStorageMode;
  cloudflare_account_id: string | null;
  cloudflare_api_token_source: CloudflareTokenSource;
  cloudflare_api_token_env_var: string;
  cloudflare_persistence_database: string;
  cloudflare_secret_store: string;
  windows_env_prefix: string;
  updated_at: string;
};

export type CloudflareEnvSnapshot = {
  account_id: string | null;
  account_id_env_var: string | null;
  account_id_env_scope: string | null;
  api_token_present: boolean;
  api_token_env_var: string | null;
  api_token_env_scope: string | null;
};

export type DependencyPreflight = {
  checks: BootstrapCheckRow[];
};

export type RuntimeDependencyState =
  | "ready"
  | "missing"
  | "outdated"
  | "misconfigured"
  | "auth_required"
  | "manual_action_required";

export type RuntimeBootstrapActionKind =
  | "install"
  | "update"
  | "authenticate"
  | "manual"
  | "retry_probe";

export type RuntimeBootstrapDisposition = "retry" | "skip" | "defer" | "cancel";

export type RuntimeDependency = {
  key: string;
  label: string;
  required: boolean;
  state: RuntimeDependencyState;
  installed_version: string | null;
  latest_version: string | null;
  resolved_path: string | null;
  detail: string;
  recommended_action_ids: string[];
};

export type RuntimeBootstrapAction = {
  action_id: string;
  dependency_key: string;
  kind: RuntimeBootstrapActionKind;
  title: string;
  description: string;
  source: string;
  command_preview: string | null;
  install_scope: string;
  requires_elevation: boolean;
  requires_interaction: boolean;
};

export type RuntimeBootstrapPlan = {
  schema_version: number;
  plan_hash: string;
  created_at: string;
  expires_at: string;
  dependencies: RuntimeDependency[];
  actions: RuntimeBootstrapAction[];
  required_ready: boolean;
  report_path: string;
  events_path: string;
};

export type RuntimeBootstrapActionResult = {
  action_id: string;
  plan_hash: string;
  status: string;
  message: string;
  command_preview: string | null;
  source: string;
  handoff_opened: boolean;
  exit_code: number | null;
  duration_ms: number | null;
  stdout: string;
  stderr: string;
  post_action_dependency: RuntimeDependency | null;
  refreshed_plan: RuntimeBootstrapPlan;
  support_bundle_path: string;
};

export type RuntimeBootstrapControlResult = {
  action_id: string;
  plan_hash: string;
  disposition: RuntimeBootstrapDisposition;
  status: string;
  recorded_at: string;
};

export type RuntimeBootstrapProgressEvent = {
  action_id: string;
  plan_hash: string;
  phase: string;
  message: string;
  at: string;
};

export type CloudflareProbeResult = {
  rows: CloudflarePermissionRow[];
};

export type CloudflareProviderStorageRequest = {
  account_id: string;
  api_token: string | null;
  api_token_env_var: string;
  persistence_database: string;
  secret_store: string;
};

export type AiProviderConfig = {
  schema_version: number;
  provider_mode: ProviderMode;
  credential_storage_mode: CredentialStorageMode;
  openai_api_key: string | null;
  anthropic_api_key: string | null;
  gemini_api_key: string | null;
  deepseek_api_key: string | null;
  grok_api_key: string | null;
  perplexity_api_key: string | null;
  openai_api_key_remote: boolean;
  anthropic_api_key_remote: boolean;
  gemini_api_key_remote: boolean;
  deepseek_api_key_remote: boolean;
  grok_api_key_remote: boolean;
  perplexity_api_key_remote: boolean;
  openai_input_usd_per_million: number | null;
  openai_output_usd_per_million: number | null;
  anthropic_input_usd_per_million: number | null;
  anthropic_output_usd_per_million: number | null;
  gemini_input_usd_per_million: number | null;
  gemini_output_usd_per_million: number | null;
  deepseek_input_usd_per_million: number | null;
  deepseek_output_usd_per_million: number | null;
  grok_input_usd_per_million: number | null;
  grok_output_usd_per_million: number | null;
  perplexity_input_usd_per_million: number | null;
  perplexity_output_usd_per_million: number | null;
  cloudflare_secret_store_id: string | null;
  cloudflare_secret_store_name: string | null;
  updated_at: string;
};

export type AiProviderProbeRow = {
  label: string;
  value: string;
  tone: "pending" | "blocked" | "ok" | "warn" | "error";
};

export type AiProviderProbeResult = {
  rows: AiProviderProbeRow[];
  checked_at: string;
};

export type LinkClassification =
  | "verified_supports_claim"
  | "verified_but_weak"
  | "redirected_verified"
  | "content_type_mismatch"
  | "not_found"
  | "forbidden"
  | "auth_required"
  | "captcha_required"
  | "paywall"
  | "timeout"
  | "dns_error"
  | "tls_error"
  | "malformed"
  | "suspected_hallucination"
  | "quarantined";

export type LinkCrossReviewStatus = "not_needed" | "pending" | "accepted" | "rejected";
export type LinkReviewDecision = "accept" | "reject" | "quarantine";
export type LinkCorrectionAction = "replace" | "remove" | "reword";

export type LinkIntegrityRedirect = {
  url: string;
  status: number;
};

export type LinkCorrectionCandidate = {
  candidate_id: string;
  action: LinkCorrectionAction;
  url: string | null;
  title: string | null;
  provider: string;
  query: string | null;
  web_evidence_id: string | null;
  rationale: string;
  proposed_at: string;
};

export type LinkIntegrityRecord = {
  schema_version: "link_evidence.v1";
  link_id: string;
  source_artifact: string;
  source_fingerprint: string;
  anchor_text: string | null;
  surrounding_text: string;
  original_url: string;
  normalized_url: string;
  normalization_changes: string[];
  final_url: string | null;
  redirect_chain: LinkIntegrityRedirect[];
  http_status: number | null;
  content_type: string | null;
  sha256: string | null;
  checked_at: string;
  claim_supported: boolean | null;
  classification: LinkClassification;
  correction_candidates: LinkCorrectionCandidate[];
  cross_review_status: LinkCrossReviewStatus;
  review_decision: LinkReviewDecision | null;
  reviewed_by: string | null;
  review_note: string | null;
  reviewed_at: string | null;
  web_evidence_id: string | null;
  url: string;
  status: string;
  invalidity: string;
  tone: "ok" | "warn" | "blocked" | "error";
};

export type LinkAuditRow = LinkIntegrityRecord;

export type LinkAuditResult = {
  urls_found: number;
  checked: number;
  ok: number;
  failed: number;
  rows: LinkIntegrityRecord[];
  schema_version: "link_integrity_audit.v1";
  audit_id: string;
  source_artifact: string;
  checked_at: string;
  pending_review: number;
  blocked: number;
};

export type LinkIntegrityListRequest = {
  query?: string;
  classifications?: LinkClassification[];
  cross_review_statuses?: LinkCrossReviewStatus[];
  source_artifact?: string;
  needs_review_only?: boolean;
  limit?: number;
  cursor?: string;
};

export type LinkIntegrityListResult = {
  items: LinkIntegrityRecord[];
  next_cursor: string | null;
  total: number;
};

export type LinkIntegrityReviewRequest = {
  link_id: string;
  decision: LinkReviewDecision;
  note: string;
  reviewer: "operator";
  expected_normalized_url: string;
  expected_sha256: string | null;
};

export type LinkCorrectionProposalRequest = {
  link_id: string;
  provider: string;
  query?: string;
  limit?: number;
};

export type CitationType =
  | "direct_quote"
  | "indirect_quote"
  | "paraphrase"
  | "apud"
  | "generic_mention";
export type CitationSourceAccess =
  | "full_document_opened"
  | "excerpt_consulted"
  | "consolidated_memory"
  | "contextual_inference"
  | "unverified_hypothesis";
export type CitationVerificationStatus = "verified" | "needs_evidence" | "quarantined";
export type CitationRisk = "low" | "medium" | "high";
export type MaestroPeerStatus = "ready" | "not_ready" | "needs_evidence";
export type CitationSourceType = "book" | "chapter" | "article" | "online" | "other";

export type CitationAuditCitation = {
  schema_version: "citation.v1";
  claim_id: string;
  citation_type: CitationType;
  author_display: string;
  author_key: string;
  year: string;
  locator: string | null;
  source_id: string;
  source_access: CitationSourceAccess;
  verification_status: CitationVerificationStatus;
  risk_if_wrong: CitationRisk;
  original_text?: string | null;
  normalized_text?: string | null;
  normalized_footnote?: string | null;
};

export type CitationAuditBlocker = {
  code: string;
  message: string;
  severity: string;
  claim_id?: string | null;
  source_id?: string | null;
  excerpt?: string | null;
  needs_evidence: boolean;
};

export type CitationAuthor = {
  author_display: string;
  author_key: string;
};

export type CitationSource = {
  source_id: string;
  source_type: CitationSourceType;
  authors: CitationAuthor[];
  title: string;
  subtitle?: string | null;
  edition?: string | null;
  place?: string | null;
  publisher?: string | null;
  year: string;
  container_title?: string | null;
  volume?: string | null;
  issue?: string | null;
  pages?: string | null;
  url?: string | null;
  doi?: string | null;
  accessed_at?: string | null;
  verification_sha256?: string | null;
  verification_status: CitationVerificationStatus;
  prohibited?: boolean;
  quarantine_reason?: string | null;
};

export type CitationManifest = {
  schema_version: "citation_manifest.v1";
  protocol_hash: string;
  citations: CitationAuditCitation[];
  sources: CitationSource[];
};

export type CitationAuditRequest = {
  text: string;
  protocol_hash?: string | null;
  manifest?: CitationManifest | null;
  previous_manifest?: CitationManifest | null;
};

export type CitationAuditResult = {
  schema_version: string;
  audit_id: string;
  checked_at: string;
  protocol_hash: string | null;
  maestro_peer_status: MaestroPeerStatus;
  citations: CitationAuditCitation[];
  normalized_references: string[];
  markdown_references: string[];
  html_references: string[];
  blockers: CitationAuditBlocker[];
  audit_table_markdown: string;
  semantic_diff: string;
};

export type WebEvidenceMethod = "GET" | "HEAD";
export type WebEvidenceAccessMode =
  | "http_fetch"
  | "rendered_fetch"
  | "official_api"
  | "operator_assisted_browser_capture";
export type WebEvidenceState =
  | "queued"
  | "collecting"
  | "ready"
  | "stale"
  | "operator_action_required"
  | "blocked"
  | "failed";
export type WebEvidenceCacheState = "fresh" | "stale" | "revalidating" | "missing";
export type WebEvidenceRobotsState = "allowed" | "disallowed" | "unavailable" | "not_applicable";
export type WebEvidenceCopyrightState = "public" | "licensed" | "operator_provided" | "unknown";
export type WebEvidenceInteractionState =
  | "none"
  | "captcha_required"
  | "login_required"
  | "consent_required"
  | "download_confirmation"
  | "paywall"
  | "human_resolved";

export type WebEvidenceRedirect = {
  url: string;
  status: number;
};

export type WebEvidenceRecord = {
  id: string;
  schema_version: "web_evidence.v1";
  state: WebEvidenceState;
  url: string;
  method: WebEvidenceMethod;
  access_mode: WebEvidenceAccessMode;
  status: number | null;
  final_url: string | null;
  title: string | null;
  content_type: string | null;
  sha256: string | null;
  retrieved_at: string | null;
  expires_at: string | null;
  cache_ttl: string;
  cache_state: WebEvidenceCacheState;
  robots_state: WebEvidenceRobotsState;
  copyright_state: WebEvidenceCopyrightState;
  interaction_state: WebEvidenceInteractionState;
  human_resolved: boolean;
  byte_count: number | null;
  duration_ms: number | null;
  redirect_chain: WebEvidenceRedirect[];
  curl_command: string | null;
  provider: string | null;
  query: string | null;
  artifact_name: string | null;
  notes: string[];
  created_at: string;
  updated_at: string;
};

export type WebEvidenceListRequest = {
  query?: string | null;
  states?: WebEvidenceState[];
  access_modes?: WebEvidenceAccessMode[];
  stale_only?: boolean;
  limit?: number;
  cursor?: string | null;
};

export type WebEvidenceListResult = {
  items: WebEvidenceRecord[];
  next_cursor: string | null;
  total: number;
};

export type WebEvidenceFetchRequest = {
  url: string;
  method: WebEvidenceMethod;
  force_revalidate: boolean;
};

export type WebEvidenceSearchRequest = {
  query: string;
  provider: string;
  limit: number;
};

export type WebEvidenceSearchResult = {
  query: string;
  provider: string;
  items: WebEvidenceRecord[];
  total: number;
};

export type WebEvidenceImportRequest = {
  url?: string | null;
  name: string;
  media_type: string;
  data_base64: string;
  notes?: string[];
};

export type WebEvidenceProgressEvent = {
  operation: string;
  evidence_id: string | null;
  phase: string;
  message: string;
  at: string;
};

export type EditorialAgentResult = {
  name: string;
  cli: string;
  tone: "ok" | "warn" | "blocked" | "error";
  status: string;
  duration_ms: number;
  exit_code: number | null;
  role: string;
  output_path: string;
  usage_input_tokens?: number | null;
  usage_output_tokens?: number | null;
  cost_usd?: number | null;
  cost_estimated?: boolean | null;
};

export type EditorialSessionResult = {
  run_id: string;
  session_dir: string;
  final_markdown_path: string | null;
  session_minutes_path: string;
  prompt_path: string;
  protocol_path: string;
  draft_path: string | null;
  agents: EditorialAgentResult[];
  consensus_ready: boolean;
  status: string;
  active_agents: InitialAgentKey[];
  max_session_cost_usd: number | null;
  max_session_minutes: number | null;
  observed_cost_usd: number | null;
  links_path: string | null;
  attachments_manifest_path: string | null;
  human_log_path: string | null;
};

export type PromptAttachmentPayload = {
  name: string;
  media_type: string | null;
  size_bytes: number;
  data_base64: string;
};

export type AttachmentDeliveryPlan = {
  attachment: PromptAttachmentPayload;
  nativeProviders: NativeAttachmentProvider[];
  manifestProviders: AiCredentialKey[];
  fallbackReason: string | null;
};

export type SessionRunOptions = {
  activeAgents: InitialAgentKey[];
  maxSessionCostUsd: number | null;
  maxSessionMinutes: number | null;
  attachments: PromptAttachmentPayload[];
  links: string[];
};

export type ResumableSessionInfo = {
  run_id: string;
  session_name: string;
  session_dir: string;
  prompt_path: string;
  protocol_path: string;
  draft_path: string | null;
  final_markdown_path: string | null;
  next_round: number;
  last_activity_unix: number;
  artifact_count: number;
  protocol_lines: number;
  status: string;
  saved_active_agents: InitialAgentKey[];
  saved_initial_agent: string | null;
  saved_max_session_cost_usd: number | null;
  saved_max_session_minutes: number | null;
};

export type ProtocolReadingGate = {
  agent: string;
  progress: number;
  status: string;
};

export type MainSiteDraft = {
  schema_version: "mainsite_draft.v1";
  requested_post_id: number | null;
  title: string;
  author: string;
  content: string;
  is_pinned: false;
  display_order: 0;
  is_published: boolean;
  is_about_site: boolean;
  sanitizer_profile: string;
  content_sha256: string;
  created_at: string;
  updated_at: string;
};

export type SaveMainSiteDraftRequest = Pick<
  MainSiteDraft,
  | "requested_post_id"
  | "title"
  | "author"
  | "content"
  | "is_published"
  | "is_about_site"
  | "sanitizer_profile"
>;
