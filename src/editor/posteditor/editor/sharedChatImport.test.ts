import { describe, expect, it } from "vitest";

import {
  getSharedChatImportDiagnostic,
  normalizeSharedChatImportResult,
  parseSharedChatUrl,
  SHARED_CHAT_URL_EXAMPLES,
  SharedChatImportError,
} from "./sharedChatImport";

describe("parseSharedChatUrl", () => {
  it.each([
    ["https://chatgpt.com/share/conversa-123", "chatgpt"],
    ["https://g.co/gemini/share/conversa_456", "gemini"],
    ["https://gemini.google.com/share/conversa-789", "gemini"],
    ["https://claude.ai/share/abcdef12-3456-7890-abcd-ef1234567890", "claude"],
  ] as const)("recognizes %s as %s", (url, provider) => {
    expect(parseSharedChatUrl(url).provider).toBe(provider);
  });

  it("publishes only safe example URLs", () => {
    expect(SHARED_CHAT_URL_EXAMPLES).toEqual([
      "https://chatgpt.com/share/exemplo",
      "https://g.co/gemini/share/exemplo",
      "https://claude.ai/share/exemplo",
    ]);
    for (const example of SHARED_CHAT_URL_EXAMPLES) {
      expect(() => parseSharedChatUrl(example)).not.toThrow();
    }
  });

  it("canonicalizes the hostname and removes query strings and fragments", () => {
    expect(
      parseSharedChatUrl(" https://www.chatgpt.com/share/conversa-123/?utm_source=teste#trecho "),
    ).toEqual({
      provider: "chatgpt",
      url: "https://chatgpt.com/share/conversa-123",
    });
  });

  it.each([
    ["", "EMPTY_URL"],
    ["não é URL", "INVALID_URL"],
    ["http://chatgpt.com/share/conversa", "HTTPS_REQUIRED"],
    ["https://usuario:senha@chatgpt.com/share/conversa", "CREDENTIALS_NOT_ALLOWED"],
    ["https://example.com/share/conversa", "UNSUPPORTED_PROVIDER"],
    ["https://chatgpt.com/c/conversa", "INVALID_SHARE_PATH"],
    ["https://gemini.google.com/app/conversa", "INVALID_SHARE_PATH"],
    ["https://claude.ai/share/%2Fsegredo", "INVALID_SHARE_PATH"],
  ] as const)("rejects %s with %s", (url, code) => {
    try {
      parseSharedChatUrl(url);
      throw new Error("A URL deveria ter sido rejeitada.");
    } catch (error) {
      expect(error).toBeInstanceOf(SharedChatImportError);
      expect((error as SharedChatImportError).code).toBe(code);
    }
  });
});

describe("normalizeSharedChatImportResult", () => {
  it("keeps only reviewed evidence metadata and strips markup from the imported title", () => {
    const normalized = normalizeSharedChatImportResult("chatgpt", {
      title: "<strong>Pesquisa</strong> &amp; revisão",
      html: "<p>Conteúdo importado</p>",
      provider: "chatgpt",
      evidence: {
        id: "evidence-123",
        source_url: "https://chatgpt.com/share/conversa-123",
        final_url: "https://chatgpt.com/share/conversa-123?interno=1",
        sha256: "a".repeat(64),
        retrieved_at: "2026-08-21T12:00:00.000Z",
        access_mode: "rendered_fetch",
        notes: ["Snapshot público", "Authorization: Bearer segredo"],
        api_token: "jamais-exportar",
      } as never,
    });

    expect(normalized.title).toBe("Pesquisa & revisão");
    expect(normalized.evidence).toEqual({
      id: "evidence-123",
      source_url: "https://chatgpt.com/share/conversa-123",
      final_url: "https://chatgpt.com/share/conversa-123",
      sha256: "a".repeat(64),
      retrieved_at: "2026-08-21T12:00:00.000Z",
      access_mode: "rendered_fetch",
      notes: ["Snapshot público"],
    });
    expect(JSON.stringify(normalized)).not.toContain("jamais-exportar");
    expect(JSON.stringify(normalized)).not.toContain("Bearer segredo");
  });

  it.each([
    ["provider divergente", { provider: "claude" }],
    ["HTML vazio", { html: "  " }],
    ["evidência ausente", { evidence: null }],
    ["evidência de outro provedor", { evidence: { source_url: "https://claude.ai/share/x" } }],
  ])("fails closed when %s", (_label, override) => {
    const result = {
      title: "Título",
      html: "<p>Conteúdo</p>",
      provider: "chatgpt" as const,
      evidence: {
        id: "evidence-1",
        source_url: "https://chatgpt.com/share/conversa",
      },
      ...override,
    };

    expect(() => normalizeSharedChatImportResult("chatgpt", result as never)).toThrow(
      SharedChatImportError,
    );
  });

  it("does not expose credentials from connector errors", () => {
    expect(getSharedChatImportDiagnostic(new Error("Authorization: Bearer segredo"))).toBe(
      "A importação falhou sem evidência suficiente para continuar. Verifique se o compartilhamento é público e tente novamente.",
    );
  });
});
