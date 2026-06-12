import { describe, expect, it } from "vitest";

import { hasUnsafeUrlScheme } from "./utils";

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

  it("allows safe and scheme-less hrefs", () => {
    expect(hasUnsafeUrlScheme("https://example.com")).toBe(false);
    expect(hasUnsafeUrlScheme("http://example.com")).toBe(false);
    expect(hasUnsafeUrlScheme("mailto:a@b.com")).toBe(false);
    expect(hasUnsafeUrlScheme("tel:+551199999")).toBe(false);
    expect(hasUnsafeUrlScheme("/relative/path")).toBe(false);
    expect(hasUnsafeUrlScheme("#anchor")).toBe(false);
    expect(hasUnsafeUrlScheme("//cdn.example.com/x.js")).toBe(false);
    expect(hasUnsafeUrlScheme("")).toBe(false);
  });
});
