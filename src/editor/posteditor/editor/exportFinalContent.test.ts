import { invoke, isTauri } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  buildFinalContentExport,
  buildPrintDocument,
  downloadExportArtifact,
  htmlToCitationAuditMarkdown,
  htmlToLinkAuditMarkdown,
  openFinalContentPrintDialog,
  sanitizeExportFilename,
} from "./exportFinalContent";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: vi.fn(() => false) }));

afterEach(() => {
  window.dispatchEvent(new Event("afterprint"));
  document.getElementById("test-workbench")?.remove();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.mocked(isTauri).mockReturnValue(false);
  vi.mocked(invoke).mockReset();
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

describe("portable desktop export custody", () => {
  it("awaits native storage of exact article and provenance instead of downloading into Windows Downloads", async () => {
    const exported = buildFinalContentExport(input, "html");
    const createObjectURL = vi.fn(() => "blob:export-fixture");
    const revokeObjectURL = vi.fn();
    const NativeURL = URL;
    vi.stubGlobal(
      "URL",
      class extends NativeURL {
        static override createObjectURL = createObjectURL;
        static override revokeObjectURL = revokeObjectURL;
      },
    );
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    vi.mocked(isTauri).mockReturnValue(true);
    let resolve!: () => void;
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise<void>((finish) => {
          resolve = finish;
        }),
    );
    let completed = false;
    const pending = Promise.resolve(downloadExportArtifact(exported.content)).then(() => {
      completed = true;
    });
    await Promise.resolve();
    expect(invoke).toHaveBeenCalledWith("persist_editor_export", {
      request: {
        format: "html",
        filename: exported.content.filename,
        content: exported.content.content,
      },
    });
    expect(completed).toBe(false);
    expect(createObjectURL).not.toHaveBeenCalled();
    resolve();
    await pending;
    await downloadExportArtifact(exported.provenance);
    expect(invoke).toHaveBeenLastCalledWith("persist_editor_export", {
      request: {
        format: "html",
        filename: exported.provenance.filename,
        content: exported.provenance.content,
      },
    });
    expect(revokeObjectURL).not.toHaveBeenCalled();
  });

  it("surfaces native disk failures without a browser download fallback", async () => {
    vi.mocked(isTauri).mockReturnValue(true);
    vi.mocked(invoke).mockRejectedValueOnce(new Error("portable folder is read-only"));
    const exported = buildFinalContentExport(input, "markdown");
    await expect(
      Promise.resolve().then(() => downloadExportArtifact(exported.content)),
    ).rejects.toThrow("portable folder is read-only");
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
  it("waits for uncached projected images and embedded frames before requesting print", async () => {
    vi.spyOn(HTMLImageElement.prototype, "complete", "get").mockReturnValue(false);
    const print = vi.spyOn(window, "print").mockImplementation(() => {});
    const pending = openFinalContentPrintDialog(
      {
        ...input,
        html: '<p>Media article</p><img src="https://example.org/uncached.png"><iframe src="https://www.youtube.com/embed/abc123" loading="lazy"></iframe>',
      },
      window,
    );
    const projection = document.getElementById("maestro-final-content-print");
    const image = projection?.querySelector("img");
    const frame = projection?.querySelector("iframe");
    expect(image).not.toBeNull();
    expect(frame).not.toBeNull();
    expect(image?.loading).toBe("eager");
    expect(frame?.loading).toBe("eager");
    expect(print).not.toHaveBeenCalled();
    image?.dispatchEvent(new Event("load"));
    await Promise.resolve();
    expect(print).not.toHaveBeenCalled();
    frame?.dispatchEvent(new Event("load"));
    await pending;
    expect(print).toHaveBeenCalledOnce();
  });

  it("waits for cached image decoding instead of treating complete as decoded", async () => {
    vi.spyOn(HTMLImageElement.prototype, "complete", "get").mockReturnValue(true);
    let resolveDecode!: () => void;
    const decode = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveDecode = resolve;
        }),
    );
    const print = vi.spyOn(window, "print").mockImplementation(() => {});
    const append = document.body.append.bind(document.body);
    vi.spyOn(document.body, "append").mockImplementation((...nodes) => {
      const projection = nodes[0] as HTMLElement;
      Object.defineProperty(projection.querySelector("img"), "decode", { value: decode });
      append(...nodes);
    });
    const pending = openFinalContentPrintDialog(
      { ...input, html: '<img src="https://example.org/cached.png" loading="lazy">' },
      window,
    );
    expect(decode).toHaveBeenCalledOnce();
    expect(print).not.toHaveBeenCalled();
    document.querySelector("#maestro-final-content-print img")?.dispatchEvent(new Event("load"));
    expect(decode).toHaveBeenCalledOnce();
    resolveDecode();
    await pending;
    expect(print).toHaveBeenCalledOnce();
  });

  it("settles failed resources and rejected image decoding without an unbounded wait", async () => {
    vi.spyOn(HTMLImageElement.prototype, "complete", "get").mockReturnValue(false);
    const print = vi.spyOn(window, "print").mockImplementation(() => {});
    const pending = openFinalContentPrintDialog(
      {
        ...input,
        html: '<img src="https://example.org/broken.png"><img src="https://example.org/undecodable.png"><iframe src="https://www.youtube.com/embed/abc123"></iframe>',
      },
      window,
    );
    const images = document.querySelectorAll("#maestro-final-content-print img");
    Object.defineProperty(images[1], "decode", {
      value: vi.fn().mockRejectedValue(new Error("Invalid image")),
    });
    images[0]?.dispatchEvent(new Event("error"));
    images[1]?.dispatchEvent(new Event("load"));
    await Promise.resolve();
    expect(print).not.toHaveBeenCalled();
    document
      .querySelector("#maestro-final-content-print iframe")
      ?.dispatchEvent(new Event("error"));
    await pending;
    expect(print).toHaveBeenCalledOnce();
  });

  it("fails a stalled media preparation after a bounded deadline and removes late listeners", async () => {
    vi.useFakeTimers();
    vi.spyOn(HTMLImageElement.prototype, "complete", "get").mockReturnValue(false);
    const initialTitle = document.title;
    const print = vi.spyOn(window, "print").mockImplementation(() => {});
    const pending = openFinalContentPrintDialog(
      { ...input, html: '<img src="https://example.org/stalled.png">' },
      window,
    );
    const rejected = expect(pending).rejects.toThrow("As mídias não terminaram de carregar");
    const image = document.querySelector("#maestro-final-content-print img");
    await vi.advanceTimersByTimeAsync(29_999);
    expect(print).not.toHaveBeenCalled();
    expect(image?.isConnected).toBe(true);
    await vi.advanceTimersByTimeAsync(1);
    await rejected;
    expect(document.getElementById("maestro-final-content-print")).toBeNull();
    expect(document.title).toBe(initialTitle);
    image?.dispatchEvent(new Event("load"));
    await Promise.resolve();
    expect(print).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
    await openFinalContentPrintDialog(input, window);
    expect(print).toHaveBeenCalledOnce();
  });

  it("keeps request ownership while media is pending and cancels preparation on pagehide", async () => {
    vi.spyOn(HTMLImageElement.prototype, "complete", "get").mockReturnValue(false);
    const initialTitle = document.title;
    const print = vi.spyOn(window, "print").mockImplementation(() => {});
    const pending = openFinalContentPrintDialog(
      { ...input, html: '<img src="https://example.org/pending.png">' },
      window,
    );
    const rejected = expect(pending).rejects.toThrow("A preparação da impressão foi cancelada");
    const projection = document.getElementById("maestro-final-content-print");
    const image = projection?.querySelector("img");
    await expect(
      openFinalContentPrintDialog({ ...input, title: "Other article" }, window),
    ).rejects.toThrow("já está em andamento");
    expect(document.getElementById("maestro-final-content-print")).toBe(projection);
    expect(document.title).toBe(input.title);
    window.dispatchEvent(new Event("pagehide"));
    await rejected;
    expect(document.title).toBe(initialTitle);
    expect(projection?.isConnected).toBe(false);
    image?.dispatchEvent(new Event("load"));
    expect(print).not.toHaveBeenCalled();
  });

  it("does not print after cleanup wins the resolved-media await continuation", async () => {
    const print = vi.spyOn(window, "print").mockImplementation(() => {});
    const pending = openFinalContentPrintDialog(input, window);
    const rejected = expect(pending).rejects.toThrow("A preparação da impressão foi cancelada");
    window.dispatchEvent(new Event("pagehide"));
    await rejected;
    expect(document.getElementById("maestro-final-content-print")).toBeNull();
    expect(print).not.toHaveBeenCalled();
  });

  it("prints only the sanitized article through the existing window even when popups are denied", async () => {
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

    await openFinalContentPrintDialog(input, window);

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

  it("cleans up a failed native print request and preserves the workbench title", async () => {
    const initialTitle = document.title;
    vi.spyOn(window, "open").mockReturnValue(null);
    vi.spyOn(window, "print").mockImplementation(() => {
      throw new Error("Native print unavailable");
    });

    await expect(openFinalContentPrintDialog(input, window)).rejects.toThrow(
      "Native print unavailable",
    );
    expect(document.querySelector('[data-maestro-export="pdf-print"]')).toBeNull();
    expect(document.title).toBe(initialTitle);
  });

  it("retires an ignored print request before the next article without accumulating hidden documents", async () => {
    const initialTitle = document.title;
    vi.spyOn(window, "open").mockReturnValue(null);
    const snapshots: string[] = [];
    vi.spyOn(window, "print").mockImplementation(() => {
      snapshots.push(
        document.querySelector('section[data-maestro-export="pdf-print"]')?.textContent ?? "",
      );
    });

    await openFinalContentPrintDialog({ ...input, html: "<p>FIRST_PRINT_SNAPSHOT</p>" }, window);
    await openFinalContentPrintDialog(
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

  it("rejects a reentrant print call without replacing the document currently being captured", async () => {
    vi.spyOn(window, "open").mockReturnValue(null);
    let rejected: Promise<unknown> | undefined;
    vi.spyOn(window, "print").mockImplementation(() => {
      rejected = expect(
        openFinalContentPrintDialog({ ...input, title: "Reentrant article" }, window),
      ).rejects.toThrow("Uma solicitação de impressão já está em andamento.");
      expect(document.title).toBe(input.title);
      expect(
        document.querySelector('section[data-maestro-export="pdf-print"]')?.textContent,
      ).toContain(input.title);
    });

    await openFinalContentPrintDialog(input, window);
    await rejected;
    window.dispatchEvent(new Event("afterprint"));
    expect(document.querySelector('[data-maestro-export="pdf-print"]')).toBeNull();
  });
});
