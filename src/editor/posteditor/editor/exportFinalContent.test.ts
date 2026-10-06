import { afterEach, describe, expect, it, vi } from "vitest";

import {
  buildFinalContentExport,
  buildPrintDocument,
  htmlToCitationAuditMarkdown,
  htmlToLinkAuditMarkdown,
  openFinalContentPrintDialog,
  sanitizeExportFilename,
} from "./exportFinalContent";

afterEach(() => {
  window.dispatchEvent(new Event("afterprint"));
  document.getElementById("test-workbench")?.remove();
  vi.restoreAllMocks();
});

describe("htmlToCitationAuditMarkdown", () => {
  it("turns editor HTML into visible citation text and a References heading", () => {
    const result = htmlToCitationAuditMarkdown(
      "<p>Silva (2026) descreve o resultado.</p><h2>Referências</h2><p><em>SILVA</em>. Obra. 2026.</p><table><tr><td>(Silva, 2026)</td></tr></table><script>hidden citation</script>",
    );
    expect(result).toContain("Silva (2026) descreve o resultado.");
    expect(result).toContain("## Referências");
    expect(result).toContain("*SILVA*. Obra. 2026.");
    expect(result).toContain("(Silva, 2026)");
    expect(result).not.toContain("<table");
    expect(result).not.toContain("hidden citation");
  });

  it("keeps table cells and rows separate in the citation audit", () => {
    const result = htmlToCitationAuditMarkdown(
      "<table><tbody><tr><td>Silva (2026)</td><td>descreve o resultado</td></tr><tr><td>Oliveira (2025)</td><td>confirma</td></tr></tbody></table>",
    );
    expect(result).toContain("Silva (2026) descreve o resultado");
    expect(result).toContain("Oliveira (2025) confirma");
    expect(result).not.toContain("resultadoOliveira");
  });

  it("uses the DOM URL attribute instead of its HTML entity spelling", () => {
    const result = htmlToCitationAuditMarkdown(
      '<p><a href="https://example.org/article?a=1&amp;b=2">fonte</a></p>',
    );
    expect(result).toContain("https://example.org/article?a=1&b=2");
    expect(result).not.toContain("&amp;b=2");
  });

  it("omits code examples without hiding later citations", () => {
    for (const html of [
      "<p><code>example`` (Silva, 2026)</code></p><p>(Oliveira, 2025)</p>",
      "<p><code>``(Silva, 2026)``</code></p><p>(Oliveira, 2025)</p>",
      "<pre>```\n(Silva, 2026)</pre><p>(Oliveira, 2025)</p>",
      "<p><code>\n``x</code></p><p>(Oliveira, 2025)</p>",
      "<ul><li><pre>~~~</pre></li></ul><p>(Oliveira, 2025)</p>",
      "<p><code></code>(Oliveira, 2025)<code></code></p>",
    ]) {
      const result = htmlToCitationAuditMarkdown(html);
      expect(result).not.toContain("(Silva, 2026)");
      expect(result).toContain("(Oliveira, 2025)");
    }
  });

  it("escapes tilde fences and collapses HTML whitespace in citation text", () => {
    expect(htmlToCitationAuditMarkdown("<p>~~~</p><p>(Silva, 2026)</p>")).toBe(
      "\\~\\~\\~\n\n(Silva, 2026)",
    );
    expect(htmlToCitationAuditMarkdown("<p><s>~</s></p><p>(Silva, 2026)</p>")).toContain(
      "(Silva, 2026)",
    );
    expect(htmlToCitationAuditMarkdown("<p>Texto.</p>    (Silva, 2026)")).toContain(
      "(Silva, 2026)",
    );
    for (const html of [
      "<p><s><s>x</s></s></p><p>(Silva, 2026)</p>",
      "<p><del><s>x</s></del></p><p>(Silva, 2026)</p>",
      "<p><s><s>x</s>y</s></p><p>(Silva, 2026)</p>",
      "<s><s>x</s></s><p>(Silva, 2026)</p>",
      '<p>A</p> <img src="https://e/x.png"> <img src="https://e/x.png"> <img src="https://e/x.png"> <img src="https://e/x.png"> (Silva, 2026)',
      "<p>A</p><sup> </sup><sup> </sup><sup> </sup><sup> </sup>(Silva, 2026)",
    ]) {
      const projected = htmlToCitationAuditMarkdown(html);
      expect(projected).toContain("(Silva, 2026)");
      expect(projected).not.toMatch(/^\s{4,}\(Silva, 2026\)/m);
      expect(projected).not.toMatch(/^~{3,}/m);
    }
  });
});

describe("htmlToLinkAuditMarkdown", () => {
  it("preserves every publishable HTML URL attribute in the link audit", () => {
    const result = htmlToLinkAuditMarkdown(
      '<figure><figcaption><a href="https://example.org/article?a=1&amp;b=2">Fonte</a></figcaption></figure><img src="https://example.org/image.png" alt="Ilustração"><iframe src="https://www.youtube.com/embed/abc123"></iframe><blockquote cite="https://example.org/quote">Trecho</blockquote>',
    );
    expect(result).toContain("https://example.org/article?a=1&b=2");
    expect(result).toContain("https://example.org/image.png");
    expect(result).toContain("https://www.youtube.com/embed/abc123");
    expect(result).toContain("https://example.org/quote");
    expect(result).not.toContain("&amp;b=2");
  });

  it("keeps a DOM href with spaces or an unmatched parenthesis parseable", () => {
    const result = htmlToLinkAuditMarkdown(
      '<a href="https://example.org/a b">Espaço</a><a href="https://example.org/a)b">Parêntese</a>',
    );
    expect(result).toContain("[Espaço](<https://example.org/a b>)");
    expect(result).toContain("[Parêntese](<https://example.org/a)b>)");
  });
});

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
  it("uses collision-free code delimiters and escapes prose tildes in Markdown exports", () => {
    const result = buildFinalContentExport(
      { ...input, html: "<p><code>example`` (Silva, 2026)</code></p><p>~~~</p>" },
      "markdown",
    );
    expect(result.content.content).toContain("```example`` (Silva, 2026)```");
    expect(result.content.content).toContain("\\~\\~\\~");
  });

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
        author: "Leonardo <script>alert(1)</script>",
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

  it("escapes backslashes, quotes and line breaks in Markdown link titles", () => {
    const result = buildFinalContentExport(
      {
        ...input,
        html: '<p><a href="https://example.com" title="C:\\docs &quot;fonte&quot;&#10;linha">Fonte</a><img src="https://example.com/capa.png" alt="Capa" title="D:\\img &quot;capa&quot;"></p>',
      },
      "markdown",
    );

    expect(result.content.content).toContain(
      '[Fonte](<https://example.com> "C:\\\\docs \\"fonte\\" linha")',
    );
    expect(result.content.content).toContain(
      '![Capa](<https://example.com/capa.png> "D:\\\\img \\"capa\\"")',
    );
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
    expect(document).toContain("Leonardo &lt;script&gt;alert(1)&lt;/script&gt;");
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

    expect(document).toContain(
      "<title>&lt;img src=x onerror=&quot;alert(1)&quot;&gt;Título &amp; revisão</title>",
    );
    expect(document).toContain(
      "<p>Autoria: &lt;a href=&quot;javascript:alert(1)&quot;&gt;Autoria&lt;/a&gt;</p>",
    );
    expect(document).not.toContain("<img src=x");
    expect(document).not.toContain('<a href="javascript:');
  });
});

describe("native final-content printing", () => {
  it("prints only the sanitized article through the existing window even when popups are denied", () => {
    const workbench = document.createElement("div");
    workbench.id = "test-workbench";
    workbench.innerHTML = "<button>PRIVATE_EDITOR_CONTROLS</button><aside>PRIVATE_SIDEBAR</aside>";
    document.body.append(workbench);
    const initialTitle = document.title;
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    const print = vi.spyOn(window, "print").mockImplementation(() => {
      const projection = document.querySelector('section[data-maestro-export="pdf-print"]');
      expect(projection).not.toBeNull();
      expect(projection?.textContent).toContain(input.title);
      expect(projection?.textContent).toContain("Resultados");
      expect(projection?.innerHTML).not.toContain("<script");
      expect(projection?.innerHTML).not.toContain("onclick");
      expect(projection?.textContent).not.toContain("PRIVATE_");
      expect(projection?.innerHTML).not.toContain(sharedChatEvidence.source_url);
      expect(document.title).toBe(input.title);
    });

    openFinalContentPrintDialog(input, window);

    expect(open).not.toHaveBeenCalled();
    expect(print).toHaveBeenCalledOnce();
    expect(document.querySelector('[data-maestro-export="pdf-print"]')).not.toBeNull();
    expect(workbench.isConnected).toBe(true);
    window.dispatchEvent(new Event("afterprint"));
    expect(document.querySelector('[data-maestro-export="pdf-print"]')).toBeNull();
    expect(document.querySelector('style[data-maestro-export="pdf-print"]')).toBeNull();
    expect(document.title).toBe(initialTitle);
    expect(workbench.isConnected).toBe(true);
  });

  it("cleans up a failed native print request and preserves the workbench title", () => {
    const initialTitle = document.title;
    vi.spyOn(window, "open").mockReturnValue(null);
    vi.spyOn(window, "print").mockImplementation(() => {
      throw new Error("Native print unavailable");
    });

    expect(() => openFinalContentPrintDialog(input, window)).toThrow("Native print unavailable");
    expect(document.querySelector('[data-maestro-export="pdf-print"]')).toBeNull();
    expect(document.title).toBe(initialTitle);
  });

  it("retires an ignored print request before the next article without accumulating hidden documents", () => {
    const initialTitle = document.title;
    vi.spyOn(window, "open").mockReturnValue(null);
    const snapshots: string[] = [];
    vi.spyOn(window, "print").mockImplementation(() => {
      snapshots.push(
        document.querySelector('section[data-maestro-export="pdf-print"]')?.textContent ?? "",
      );
    });

    openFinalContentPrintDialog({ ...input, html: "<p>FIRST_PRINT_SNAPSHOT</p>" }, window);
    openFinalContentPrintDialog(
      { ...input, title: "Second article", html: "<p>SECOND_PRINT_SNAPSHOT</p>" },
      window,
    );

    expect(snapshots).toHaveLength(2);
    expect(snapshots[0]).toContain("FIRST_PRINT_SNAPSHOT");
    expect(snapshots[0]).not.toContain("SECOND_PRINT_SNAPSHOT");
    expect(snapshots[1]).toContain("SECOND_PRINT_SNAPSHOT");
    expect(snapshots[1]).not.toContain("FIRST_PRINT_SNAPSHOT");
    expect(document.querySelectorAll('section[data-maestro-export="pdf-print"]')).toHaveLength(1);
    expect(document.querySelectorAll('style[data-maestro-export="pdf-print"]')).toHaveLength(1);
    window.dispatchEvent(new Event("afterprint"));
    expect(document.querySelector('[data-maestro-export="pdf-print"]')).toBeNull();
    expect(document.title).toBe(initialTitle);
  });

  it("rejects a reentrant print call without replacing the document currently being captured", () => {
    vi.spyOn(window, "open").mockReturnValue(null);
    vi.spyOn(window, "print").mockImplementation(() => {
      expect(() =>
        openFinalContentPrintDialog({ ...input, title: "Reentrant article" }, window),
      ).toThrow("Uma solicitação de impressão já está em andamento.");
      expect(document.title).toBe(input.title);
      expect(
        document.querySelector('section[data-maestro-export="pdf-print"]')?.textContent,
      ).toContain(input.title);
    });

    openFinalContentPrintDialog(input, window);
    window.dispatchEvent(new Event("afterprint"));
    expect(document.querySelector('[data-maestro-export="pdf-print"]')).toBeNull();
  });
});
