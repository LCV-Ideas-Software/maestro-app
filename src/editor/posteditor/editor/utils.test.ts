import { describe, expect, it } from "vitest";

import { hasUnsafeUrlScheme, sanitizeMainSiteLinks } from "./utils";

describe("hasUnsafeUrlScheme", () => {
  it("rejects script-capable schemes", () => {
    expect(hasUnsafeUrlScheme("javascript:alert(1)")).toBe(true);
    expect(hasUnsafeUrlScheme("JaVaScRiPt:alert(1)")).toBe(true);
    expect(hasUnsafeUrlScheme("data:text/html,<script>1</script>")).toBe(true);
    expect(hasUnsafeUrlScheme("vbscript:msgbox(1)")).toBe(true);
    expect(hasUnsafeUrlScheme("file:///etc/passwd")).toBe(true);
  });

  it("ignores whitespace/control chars browsers strip from the scheme", () => {
    expect(hasUnsafeUrlScheme("java\tscript:alert(1)")).toBe(true);
    expect(hasUnsafeUrlScheme("  javascript:alert(1)")).toBe(true);
    expect(hasUnsafeUrlScheme("java\nscript:alert(1)")).toBe(true);
  });

  it("allows only audited protocols and scheme-less hrefs", () => {
    expect(hasUnsafeUrlScheme("https://example.com")).toBe(false);
    expect(hasUnsafeUrlScheme("http://example.com")).toBe(false);
    expect(hasUnsafeUrlScheme("mailto:a@b.com")).toBe(false);
    expect(hasUnsafeUrlScheme("tel:+551199999")).toBe(true);
    expect(hasUnsafeUrlScheme("ftp://files.example.com/x")).toBe(true);
    expect(hasUnsafeUrlScheme("ftps://files.example.com/x")).toBe(true);
    expect(hasUnsafeUrlScheme("/relative/path")).toBe(false);
    expect(hasUnsafeUrlScheme("#anchor")).toBe(false);
    expect(hasUnsafeUrlScheme("//cdn.example.com/x.js")).toBe(false);
    expect(hasUnsafeUrlScheme("")).toBe(false);
  });
});

describe("sanitizeMainSiteLinks", () => {
  it("removes dangerous or control-obfuscated hrefs without changing anchor text", () => {
    const sanitized = sanitizeMainSiteLinks(
      '<p><a href="javascript:alert(1)" target="_blank">script</a>' +
        '<a href="java&#9;script:alert(1)">control</a></p>',
    );

    expect(sanitized).toContain("<a>script</a>");
    expect(sanitized).toContain("<a>control</a>");
    expect(sanitized).not.toContain("javascript");
    expect(sanitized).not.toContain("target");
  });

  it("forces target and rel on every safe non-YouTube link", () => {
    const sanitized = sanitizeMainSiteLinks(
      '<a href="https://example.com/source">web</a>' +
        '<a href="http://example.com/other">http</a>' +
        '<a href="mailto:editor@example.com">mail</a>' +
        '<a href="/artigos/1">internal</a>' +
        '<a href="#referencias">fragment</a>' +
        '<a href="//cdn.example.com/source">scheme relative</a>',
    );

    expect(sanitized).toContain(
      '<a href="https://example.com/source" target="_blank" rel="noopener noreferrer">web</a>',
    );
    expect(sanitized).toContain(
      '<a href="http://example.com/other" target="_blank" rel="noopener noreferrer">http</a>',
    );
    expect(sanitized).toContain(
      '<a href="mailto:editor@example.com" target="_blank" rel="noopener noreferrer">mail</a>',
    );
    expect(sanitized).toContain(
      '<a href="/artigos/1" target="_blank" rel="noopener noreferrer">internal</a>',
    );
    expect(sanitized).toContain(
      '<a href="#referencias" target="_blank" rel="noopener noreferrer">fragment</a>',
    );
    expect(sanitized).toContain(
      '<a href="//cdn.example.com/source" target="_blank" rel="noopener noreferrer">scheme relative</a>',
    );
  });

  it("keeps YouTube link attributes unchanged", () => {
    const original = '<a href="https://www.youtube.com/watch?v=abc">vídeo</a>';

    expect(sanitizeMainSiteLinks(original)).toBe(original);
  });

  it("is idempotent", () => {
    const once = sanitizeMainSiteLinks('<a href="https://example.com">fonte</a>');

    expect(sanitizeMainSiteLinks(once)).toBe(once);
  });
});
