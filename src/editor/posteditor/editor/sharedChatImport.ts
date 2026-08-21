export type SharedChatProvider = "chatgpt" | "gemini" | "claude";

export type SharedChatEvidenceMetadata = {
  id: string;
  source_url: string;
  final_url?: string | null;
  sha256?: string | null;
  retrieved_at?: string | null;
  access_mode?: string | null;
  notes?: string[];
};

export type StoredSharedChatEvidence = SharedChatEvidenceMetadata & {
  provider: SharedChatProvider;
};

export type SharedChatImportResult = {
  title: string | null;
  html: string;
  provider: SharedChatProvider;
  evidence: SharedChatEvidenceMetadata;
};

export type SharedChatImporter = (url: string) => Promise<SharedChatImportResult>;

export type SharedChatImportErrorCode =
  | "EMPTY_URL"
  | "INVALID_URL"
  | "HTTPS_REQUIRED"
  | "CREDENTIALS_NOT_ALLOWED"
  | "UNSUPPORTED_PROVIDER"
  | "INVALID_SHARE_PATH"
  | "CONNECTOR_UNAVAILABLE"
  | "INVALID_RESPONSE"
  | "PROVIDER_MISMATCH"
  | "EVIDENCE_MISSING";

export class SharedChatImportError extends Error {
  readonly code: SharedChatImportErrorCode;

  constructor(code: SharedChatImportErrorCode, message: string) {
    super(message);
    this.name = "SharedChatImportError";
    this.code = code;
  }
}

export const SHARED_CHAT_URL_EXAMPLES = [
  "https://chatgpt.com/share/exemplo",
  "https://g.co/gemini/share/exemplo",
  "https://claude.ai/share/exemplo",
] as const;

const PROVIDER_LABELS: Readonly<Record<SharedChatProvider, string>> = {
  chatgpt: "ChatGPT",
  gemini: "Gemini",
  claude: "Claude",
};

const SHARE_ID = /^[a-zA-Z0-9_-]{1,256}$/;
const SENSITIVE_NOTE =
  /(?:authorization|bearer|api[\s_-]?key|api[\s_-]?token|access[\s_-]?token|secret|password|cookie)/i;

function canonicalHostname(hostname: string): string {
  return hostname.toLowerCase().replace(/^www\./, "");
}

function providerFromUrl(url: URL): SharedChatProvider | null {
  const hostname = canonicalHostname(url.hostname);
  if (hostname === "chatgpt.com") return "chatgpt";
  if (hostname === "g.co" || hostname === "gemini.google.com") return "gemini";
  if (hostname === "claude.ai") return "claude";
  return null;
}

function hasValidSharePath(url: URL, provider: SharedChatProvider): boolean {
  const hostname = canonicalHostname(url.hostname);
  const segments = url.pathname.split("/").filter(Boolean);

  if (provider === "gemini" && hostname === "g.co") {
    return (
      segments.length === 3 &&
      segments[0]?.toLowerCase() === "gemini" &&
      segments[1]?.toLowerCase() === "share" &&
      SHARE_ID.test(segments[2] ?? "")
    );
  }

  return (
    segments.length === 2 &&
    segments[0]?.toLowerCase() === "share" &&
    SHARE_ID.test(segments[1] ?? "")
  );
}

export function getSharedChatProviderLabel(provider: SharedChatProvider): string {
  return PROVIDER_LABELS[provider];
}

export function getSharedChatImportDiagnostic(error: unknown): string {
  if (error instanceof SharedChatImportError) return error.message;
  if (error instanceof Error) {
    const message = sanitizePlainText(error.message, 500);
    if (message && !SENSITIVE_NOTE.test(message)) return message;
  }
  return "A importação falhou sem evidência suficiente para continuar. Verifique se o compartilhamento é público e tente novamente.";
}

export function parseSharedChatUrl(input: string): {
  provider: SharedChatProvider;
  url: string;
} {
  const trimmed = input.trim();
  if (!trimmed) {
    throw new SharedChatImportError("EMPTY_URL", "Informe um link público de compartilhamento.");
  }

  let parsed: URL;
  try {
    parsed = new URL(trimmed);
  } catch {
    throw new SharedChatImportError("INVALID_URL", "A URL informada não é válida.");
  }

  if (parsed.protocol !== "https:") {
    throw new SharedChatImportError("HTTPS_REQUIRED", "O compartilhamento precisa usar HTTPS.");
  }
  if (parsed.username || parsed.password || parsed.port) {
    throw new SharedChatImportError(
      "CREDENTIALS_NOT_ALLOWED",
      "Links com credenciais ou porta explícita não são aceitos.",
    );
  }

  const provider = providerFromUrl(parsed);
  if (!provider) {
    throw new SharedChatImportError(
      "UNSUPPORTED_PROVIDER",
      "Use um link público de compartilhamento do ChatGPT, Gemini ou Claude.",
    );
  }
  if (!hasValidSharePath(parsed, provider)) {
    throw new SharedChatImportError(
      "INVALID_SHARE_PATH",
      `O link não corresponde a um compartilhamento público válido do ${PROVIDER_LABELS[provider]}.`,
    );
  }

  const hostname = canonicalHostname(parsed.hostname);
  const pathname = parsed.pathname.replace(/\/+$/, "");
  return { provider, url: `https://${hostname}${pathname}` };
}

function sanitizePlainText(value: unknown, maxLength: number): string {
  if (typeof value !== "string") return "";
  const document = new DOMParser().parseFromString(value, "text/html");
  return (document.body.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, maxLength);
}

function optionalIsoDate(value: unknown): string | null | undefined {
  if (value === null) return null;
  if (typeof value !== "string" || !value.trim()) return undefined;
  const normalized = value.trim();
  return Number.isNaN(Date.parse(normalized)) ? undefined : normalized;
}

function optionalSha256(value: unknown): string | null | undefined {
  if (value === null) return null;
  if (typeof value !== "string") return undefined;
  const normalized = value.trim().toLowerCase();
  return /^[a-f0-9]{64}$/.test(normalized) ? normalized : undefined;
}

export function normalizeSharedChatEvidence(
  provider: SharedChatProvider,
  evidence: SharedChatEvidenceMetadata,
): SharedChatEvidenceMetadata {
  if (!evidence || typeof evidence !== "object") {
    throw new SharedChatImportError(
      "EVIDENCE_MISSING",
      "O conector não retornou a proveniência obrigatória da importação.",
    );
  }

  const id = sanitizePlainText(evidence.id, 256);
  if (!id) {
    throw new SharedChatImportError(
      "EVIDENCE_MISSING",
      "A evidência retornada não possui identificador.",
    );
  }

  let source: ReturnType<typeof parseSharedChatUrl>;
  try {
    source = parseSharedChatUrl(evidence.source_url);
  } catch {
    throw new SharedChatImportError(
      "EVIDENCE_MISSING",
      "A evidência retornada não possui uma URL pública reconhecível.",
    );
  }
  if (source.provider !== provider) {
    throw new SharedChatImportError(
      "PROVIDER_MISMATCH",
      "A proveniência retornada pertence a outro provedor.",
    );
  }

  let finalUrl: string | null | undefined;
  if (evidence.final_url === null) {
    finalUrl = null;
  } else if (evidence.final_url) {
    const finalSource = parseSharedChatUrl(evidence.final_url);
    if (finalSource.provider !== provider) {
      throw new SharedChatImportError(
        "PROVIDER_MISMATCH",
        "A URL final da evidência pertence a outro provedor.",
      );
    }
    finalUrl = finalSource.url;
  }

  const normalized: SharedChatEvidenceMetadata = {
    id,
    source_url: source.url,
  };
  if (finalUrl !== undefined) normalized.final_url = finalUrl;

  const sha256 = optionalSha256(evidence.sha256);
  if (sha256 !== undefined) normalized.sha256 = sha256;

  const retrievedAt = optionalIsoDate(evidence.retrieved_at);
  if (retrievedAt !== undefined) normalized.retrieved_at = retrievedAt;

  const accessMode = sanitizePlainText(evidence.access_mode, 80);
  if (accessMode) normalized.access_mode = accessMode;

  const notes = Array.isArray(evidence.notes)
    ? evidence.notes
        .map((note) => sanitizePlainText(note, 500))
        .filter((note) => note && !SENSITIVE_NOTE.test(note))
        .slice(0, 20)
    : [];
  if (notes.length > 0) normalized.notes = notes;

  return normalized;
}

export function normalizeSharedChatImportResult(
  expectedProvider: SharedChatProvider,
  result: SharedChatImportResult,
): SharedChatImportResult {
  if (!result || typeof result !== "object") {
    throw new SharedChatImportError(
      "INVALID_RESPONSE",
      "O conector não retornou um resultado de importação válido.",
    );
  }
  if (result.provider !== expectedProvider) {
    throw new SharedChatImportError(
      "PROVIDER_MISMATCH",
      "O provedor retornado não corresponde ao link solicitado.",
    );
  }
  if (typeof result.html !== "string" || !result.html.trim()) {
    throw new SharedChatImportError(
      "INVALID_RESPONSE",
      "O conector não extraiu conteúdo público desse compartilhamento.",
    );
  }

  return {
    provider: result.provider,
    title: sanitizePlainText(result.title, 240) || null,
    html: result.html,
    evidence: normalizeSharedChatEvidence(expectedProvider, result.evidence),
  };
}
