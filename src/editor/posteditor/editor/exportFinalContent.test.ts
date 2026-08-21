import { describe, expect, it } from "vitest";

import {
  buildFinalContentExport,
  buildPrintDocument,
  sanitizeExportFilename,
} from "./exportFinalContent";

const sharedChatEvidence = {
  provider: "chatgpt" as const,
  id: "evidence-123",
  source_url: "https://chatgpt.com/share/conversa",
  final_url: null,
  sha256: "b".repeat(64),
  retrieved_at: "2026-08-21T11:00:00.000Z",
  access_mode: "rendered_fetch",
  notes: ["Snapshot público"],
};

const input = {
  title: "Análise de citações",
  author: "Leonardo <script>alert(1)</script>",
  html: `<h2 style="text-align: center" onclick="alert(1)">Resultados</h2>
    <p>Leia a <strong>fonte</strong> em <a href="https://example.com/artigo">Exemplo</a>.</p>
    <script>alert("conteúdo")</script>`,
  exportedAt: "2026-08-21T12:00:00.000Z",
  evidence: [sharedChatEvidence],
};

describe("sanitizeExportFilename", () => {
  it.each([
    ["Análise de citações", "analise-de-citacoes"],
    ["../../Relatório: final?.md", "relatorio-final-md"],
    ["  ", "artigo"],
    ["CON", "artigo-con"],
  ])("converts %s to %s", (title, expected) => {
    expect(sanitizeExportFilename(title)).toBe(expected);
  });
});

describe("buildFinalContentExport", () => {
  it("exports the exact sanitized MainSite fragment and separate provenance", () => {
    const result = buildFinalContentExport(input, "html");

    expect(result.content.filename).toBe("analise-de-citacoes.mainsite.html");
    expect(result.content.mimeType).toBe("text/html;charset=utf-8");
    expect(result.content.content).toContain('<h2 style="text-align: center">Resultados</h2>');
    expect(result.content.content).toContain(
      '<a href="https://example.com/artigo" rel="noopener noreferrer" target="_blank">Exemplo</a>',
    );
    expect(result.content.content).not.toContain("onclick");
    expect(result.content.content).not.toContain("script");

    expect(result.provenance.filename).toBe("analise-de-citacoes.provenance.json");
    expect(result.provenance.content).not.toContain(result.content.content);
    expect(JSON.parse(result.provenance.content)).toEqual({
      schema_version: "maestro.export-provenance.v1",
      exported_at: "2026-08-21T12:00:00.000Z",
      format: "html",
      document: {
        title: "Análise de citações",
        author: "Leonardo alert(1)",
        filename: "analise-de-citacoes.mainsite.html",
      },
      evidence: input.evidence,
    });
  });

  it("exports readable Markdown with escaped document metadata", () => {
    const result = buildFinalContentExport(
      { ...input, title: "Pesquisa [piloto] #1", author: "Autoria *editorial*" },
      "markdown",
    );

    expect(result.content.filename).toBe("pesquisa-piloto-1.md");
    expect(result.content.mimeType).toBe("text/markdown;charset=utf-8");
    expect(result.content.content).toContain("# Pesquisa \\[piloto\\] \\#1");
    expect(result.content.content).toContain("> Autoria: Autoria \\*editorial\\*");
    expect(result.content.content).toContain("## Resultados");
    expect(result.content.content).toContain("**fonte**");
    expect(result.content.content).toContain("[Exemplo](<https://example.com/artigo>)");
    expect(result.content.content).not.toContain("script");
  });

  it("never serializes unreviewed runtime properties into provenance", () => {
    const withSecret = {
      ...input,
      evidence: [
        {
          ...sharedChatEvidence,
          authorization: "Bearer segredo",
          api_token: "token-secreto",
        },
      ],
    };
    const result = buildFinalContentExport(withSecret, "html");

    expect(result.provenance.content).not.toContain("Bearer segredo");
    expect(result.provenance.content).not.toContain("token-secreto");
  });
});

describe("buildPrintDocument", () => {
  it("escapes metadata, uses sanitized final HTML and contains no executable script", () => {
    const document = buildPrintDocument(input);

    expect(document).toContain("<title>Análise de citações</title>");
    expect(document).toContain("Leonardo alert(1)");
    expect(document).not.toContain("<script");
    expect(document).not.toContain("onclick");
    expect(document).toContain('<meta name="maestro-export" content="pdf-print">');
  });

  it("cannot inject metadata into the printable document", () => {
    const document = buildPrintDocument({
      ...input,
      title: '<img src=x onerror="alert(1)">Título & revisão',
      author: '<a href="javascript:alert(1)">Autoria</a>',
    });

    expect(document).toContain("<title>Título &amp; revisão</title>");
    expect(document).toContain("<p>Autoria: Autoria</p>");
    expect(document).not.toContain("onerror");
    expect(document).not.toContain("javascript:");
  });
});
