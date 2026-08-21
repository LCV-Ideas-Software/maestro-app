import { describe, expect, it } from "vitest";

import { FINAL_HTML_FIXTURES } from "./finalHtmlFixtures";
import { convertMarkdownToFormattedHtml } from "./markdownImport";
import { sanitizeFinalMainSiteHtml, sanitizeFinalPostHtml } from "./sanitizeFinalHtml";

const parse = (html: string): Document => new DOMParser().parseFromString(html, "text/html");

describe("sanitizeFinalMainSiteHtml semantic fixtures", () => {
  it.each(FINAL_HTML_FIXTURES)("preserves $label", ({ html, expectations }) => {
    const sanitized = sanitizeFinalMainSiteHtml(html);
    const document = parse(sanitized);

    for (const expectation of expectations) {
      const element = document.querySelector(expectation.selector);
      expect(element, `elemento obrigatório: ${expectation.selector}`).not.toBeNull();
      if (!element) continue;

      if (expectation.text) expect(element.textContent).toContain(expectation.text);
      for (const [name, value] of Object.entries(expectation.attributes ?? {})) {
        expect(element.getAttribute(name), `${expectation.selector}[${name}]`).toBe(value);
      }
    }
  });

  it("round-trips an actual Markdown import through the final sanitizer", () => {
    const imported = convertMarkdownToFormattedHtml(`# Relatório Markdown

Um parágrafo com **evidência** e [fonte](https://example.com/fonte).

- Achado um
- Achado dois
`);
    const document = parse(sanitizeFinalMainSiteHtml(imported.html));

    expect(imported.title).toBe("Relatório Markdown");
    expect(document.querySelector("p strong")?.textContent).toBe("evidência");
    expect(document.querySelectorAll("ul li")).toHaveLength(2);
    expect(document.querySelector("a")?.getAttribute("target")).toBe("_blank");
    expect(document.querySelector("a")?.getAttribute("rel")).toBe("noopener noreferrer");
  });
});

describe("sanitizeFinalPostHtml allowlist", () => {
  it("preserves reviewed attributes and CSS while removing declarations outside the allowlist", () => {
    const document = parse(
      sanitizeFinalPostHtml(`<h2 style="text-align: center; color: #123456">Título</h2>
        <p style="line-height: 1.5; text-indent: 1.5rem; position: fixed; display: none">Texto</p>
        <table style="width: 100%; background-color: white"><tbody><tr><td colspan="2" colwidth="120" style="vertical-align: middle">Dado</td></tr></tbody></table>`),
    );

    expect(document.querySelector("h2")?.getAttribute("style")).toContain("text-align: center");
    expect(document.querySelector("h2")?.getAttribute("style")).toContain("color: #123456");
    expect(document.querySelector("p")?.getAttribute("style")).toBe(
      "line-height: 1.5; text-indent: 1.5rem",
    );
    expect(document.querySelector("table")?.getAttribute("style")).toBe(
      "width: 100%; background-color: white",
    );
    expect(document.querySelector("td")?.getAttribute("colspan")).toBe("2");
    expect(document.querySelector("td")?.getAttribute("colwidth")).toBe("120");
    expect(document.querySelector("td")?.getAttribute("style")).toBe("vertical-align: middle");
  });

  it("removes executable markup, unknown attributes, unsafe URLs and protocol-relative URLs", () => {
    const sanitized = sanitizeFinalPostHtml(`<article data-private="true">
      <script>alert(1)</script><style>body{display:none}</style>
      <p id="internal" onclick="alert(1)">Texto <custom-tag>mantido</custom-tag></p>
      <a href="javascript:alert(1)" target="_self">script</a>
      <a href="data:text/html,boom">data</a>
      <a href="//cdn.example.com/resource">relativo ao protocolo</a>
      <img src="file:///segredo" onerror="alert(1)">
    </article>`);
    const document = parse(sanitized);

    expect(document.querySelector("article, script, style, custom-tag")).toBeNull();
    expect(document.body.textContent).toContain("mantido");
    expect(document.querySelector("p")?.hasAttribute("id")).toBe(false);
    expect(document.querySelector("p")?.hasAttribute("onclick")).toBe(false);
    for (const anchor of document.querySelectorAll("a"))
      expect(anchor.hasAttribute("href")).toBe(false);
    expect(document.querySelector("img")?.hasAttribute("src")).toBe(false);
    expect(document.querySelector("img")?.getAttribute("loading")).toBe("lazy");
    expect(sanitized).not.toContain("alert(1)");
  });

  it("rejects CSS script primitives and keeps valid declarations from the same style", () => {
    const document = parse(
      sanitizeFinalPostHtml(
        `<p style="color: red; background-image: url(javascript:alert(1)); width: expression(alert(1)); font-weight: 700; display: inline-block">Texto</p>`,
      ),
    );

    expect(document.querySelector("p")?.getAttribute("style")).toBe(
      "color: red; font-weight: 700; display: inline-block",
    );
  });

  it("accepts only YouTube embed iframes", () => {
    const document = parse(
      sanitizeFinalPostHtml(`<iframe src="https://www.youtube.com/embed/safe" title="Seguro"></iframe>
        <iframe src="https://www.youtube.com/watch?v=not-an-embed" title="Rota errada"></iframe>
        <iframe src="https://example.com/embed/unsafe" title="Host errado"></iframe>
        <iframe src="javascript:alert(1)" title="Script"></iframe>`),
    );
    const frames = document.querySelectorAll("iframe");

    expect(frames).toHaveLength(1);
    expect(frames[0]?.getAttribute("src")).toBe("https://www.youtube.com/embed/safe");
    expect(frames[0]?.getAttribute("title")).toBe("Seguro");
  });

  it("defaults images to lazy loading and preserves only explicit eager loading", () => {
    const document = parse(
      sanitizeFinalPostHtml(`<img src="https://example.com/lazy.png">
        <img src="https://example.com/eager.png" loading="eager">
        <img src="https://example.com/invalid.png" loading="auto">`),
    );
    const images = document.querySelectorAll("img");

    expect(images[0]?.getAttribute("loading")).toBe("lazy");
    expect(images[1]?.getAttribute("loading")).toBe("eager");
    expect(images[2]?.getAttribute("loading")).toBe("lazy");
  });

  it("is idempotent and returns empty output for empty or invalid runtime input", () => {
    const once = sanitizeFinalPostHtml(
      '<p style="text-align: justify">Texto <a href="https://example.com">fonte</a></p>',
    );

    expect(sanitizeFinalPostHtml(once)).toBe(once);
    expect(sanitizeFinalPostHtml("")).toBe("");
    expect(sanitizeFinalPostHtml(null as unknown as string)).toBe("");
  });
});
