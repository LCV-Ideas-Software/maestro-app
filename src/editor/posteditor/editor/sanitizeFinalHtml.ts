import DOMPurify from "dompurify";
import { hasUnsafeUrlScheme, sanitizeMainSiteLinks } from "./utils";

const LENGTH_NON_NEGATIVE = /^(?:0|\d+(?:\.\d+)?(?:px|em|rem|%|pt|vh|vw))$/i;
const LINE_HEIGHT = /^(?:normal|\d+(?:\.\d+)?(?:px|em|rem|%)?)$/i;
const FONT_FAMILY = /^[a-zA-Z0-9\s,'"_-]+$/;
const COLOR_HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const COLOR_RGB =
  /^rgba?\(\s*\d{1,3}\s*,\s*\d{1,3}\s*,\s*\d{1,3}\s*(?:,\s*(?:0|1|0?\.\d+)\s*)?\)$/i;
const COLOR_HSL =
  /^hsla?\(\s*\d{1,3}\s*,\s*\d{1,3}%\s*,\s*\d{1,3}%\s*(?:,\s*(?:0|1|0?\.\d+)\s*)?\)$/i;
const COLOR_NAMED = /^[a-z]+$/i;
const SAFE_DIMENSION = /^(?:auto|0|\d+(?:\.\d+)?(?:px|em|rem|%|vw|vh))$/i;
const SAFE_DISPLAY = /^(?:block|inline|inline-block|flex|inline-flex)$/i;
const TEXT_ALIGN = /^(?:left|right|center|justify|start|end)$/i;
const SAFE_IFRAME_HOSTS = new Set([
  "www.youtube.com",
  "youtube.com",
  "www.youtube-nocookie.com",
  "youtube-nocookie.com",
]);

const ALLOWED_TAGS = [
  "a",
  "abbr",
  "b",
  "blockquote",
  "br",
  "caption",
  "code",
  "col",
  "colgroup",
  "del",
  "div",
  "em",
  "figcaption",
  "figure",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "hr",
  "i",
  "iframe",
  "img",
  "input",
  "label",
  "li",
  "mark",
  "ol",
  "p",
  "pre",
  "s",
  "span",
  "strong",
  "sub",
  "sup",
  "table",
  "tbody",
  "td",
  "tfoot",
  "th",
  "thead",
  "tr",
  "u",
  "ul",
] as const;

const HEADING_ATTRIBUTES = new Set(["style"]);
const ATTRIBUTES_BY_TAG: Readonly<Record<string, ReadonlySet<string>>> = {
  a: new Set(["href", "name", "target", "rel", "title"]),
  blockquote: new Set(["cite"]),
  code: new Set(["class"]),
  col: new Set(["span", "style", "width"]),
  colgroup: new Set(["span", "width"]),
  div: new Set(["class", "data-youtube-video", "style"]),
  figure: new Set(["class", "style"]),
  h1: HEADING_ATTRIBUTES,
  h2: HEADING_ATTRIBUTES,
  h3: HEADING_ATTRIBUTES,
  h4: HEADING_ATTRIBUTES,
  h5: HEADING_ATTRIBUTES,
  h6: HEADING_ATTRIBUTES,
  iframe: new Set([
    "allow",
    "allowfullscreen",
    "frameborder",
    "height",
    "scrolling",
    "src",
    "style",
    "title",
    "width",
  ]),
  img: new Set(["alt", "data-width", "height", "loading", "src", "style", "title", "width"]),
  input: new Set(["checked", "disabled", "type"]),
  label: new Set(["for"]),
  li: new Set(["data-checked", "data-type", "style"]),
  mark: new Set(["style"]),
  ol: new Set(["start", "style", "type"]),
  p: new Set(["style"]),
  pre: new Set(["class"]),
  span: new Set(["class", "style"]),
  table: new Set(["style", "width"]),
  td: new Set(["colspan", "colwidth", "rowspan", "style"]),
  th: new Set(["colspan", "colwidth", "rowspan", "scope", "style"]),
  ul: new Set(["data-type", "style"]),
};

const ALLOWED_ATTRIBUTES = [
  ...new Set(Object.values(ATTRIBUTES_BY_TAG).flatMap((attributes) => [...attributes])),
];

const STYLE_RULES: Readonly<Record<string, readonly RegExp[]>> = {
  "background-color": [COLOR_HEX, COLOR_RGB, COLOR_HSL, COLOR_NAMED],
  color: [COLOR_HEX, COLOR_RGB, COLOR_HSL, COLOR_NAMED],
  display: [SAFE_DISPLAY],
  "font-family": [FONT_FAMILY],
  "font-size": [LENGTH_NON_NEGATIVE],
  "font-style": [/^(?:normal|italic|oblique)$/i],
  "font-weight": [/^(?:normal|bold|bolder|lighter|[1-9]00)$/i],
  height: [SAFE_DIMENSION],
  "line-height": [LINE_HEIGHT],
  "margin-bottom": [LENGTH_NON_NEGATIVE],
  "margin-top": [LENGTH_NON_NEGATIVE],
  "max-width": [SAFE_DIMENSION],
  "min-width": [SAFE_DIMENSION],
  "text-align": [TEXT_ALIGN],
  "text-decoration": [/^(?:none|underline|line-through|overline)$/i],
  "text-indent": [LENGTH_NON_NEGATIVE],
  "vertical-align": [/^(?:baseline|sub|super|top|middle|bottom)$/i],
  width: [SAFE_DIMENSION],
};

function sanitizeStyle(style: string): string {
  const safeDeclarations: string[] = [];
  for (const declaration of style.split(";")) {
    const separator = declaration.indexOf(":");
    if (separator < 1) continue;
    const property = declaration.slice(0, separator).trim().toLowerCase();
    const value = declaration.slice(separator + 1).trim();
    const rules = STYLE_RULES[property];
    if (!value || !rules?.some((rule) => rule.test(value))) continue;
    safeDeclarations.push(`${property}: ${value}`);
  }
  return safeDeclarations.join("; ");
}

function isSafePublishableUrl(value: string, allowMailto: boolean): boolean {
  const trimmed = value.trim();
  if (!trimmed || trimmed.startsWith("//") || hasUnsafeUrlScheme(trimmed)) return false;
  if (!allowMailto && /^mailto:/i.test(trimmed)) return false;
  return true;
}

function hasAllowedYoutubeSource(value: string): boolean {
  try {
    const parsed = new URL(value);
    return (
      (parsed.protocol === "https:" || parsed.protocol === "http:") &&
      SAFE_IFRAME_HOSTS.has(parsed.hostname.toLowerCase()) &&
      parsed.pathname.startsWith("/embed/")
    );
  } catch {
    return false;
  }
}

function enforceTagAttributeAllowlist(document: Document): void {
  for (const element of document.body.querySelectorAll<HTMLElement>("*")) {
    const tag = element.tagName.toLowerCase();
    const allowedAttributes = ATTRIBUTES_BY_TAG[tag] ?? new Set<string>();
    for (const attribute of [...element.attributes]) {
      if (!allowedAttributes.has(attribute.name.toLowerCase())) {
        element.removeAttribute(attribute.name);
      }
    }

    const style = element.getAttribute("style");
    if (style !== null) {
      const sanitizedStyle = sanitizeStyle(style);
      if (sanitizedStyle) element.setAttribute("style", sanitizedStyle);
      else element.removeAttribute("style");
    }
  }
}

function enforceUrlAndTransformRules(document: Document): void {
  for (const anchor of document.body.querySelectorAll<HTMLAnchorElement>("a[href]")) {
    const href = anchor.getAttribute("href") ?? "";
    if (!isSafePublishableUrl(href, true)) anchor.removeAttribute("href");
    anchor.setAttribute("rel", "noopener noreferrer");
  }

  for (const image of document.body.querySelectorAll<HTMLImageElement>("img")) {
    const src = image.getAttribute("src");
    if (src !== null && !isSafePublishableUrl(src, false)) image.removeAttribute("src");
    image.setAttribute("loading", image.getAttribute("loading") === "eager" ? "eager" : "lazy");
  }

  for (const frame of document.body.querySelectorAll<HTMLIFrameElement>("iframe")) {
    const src = frame.getAttribute("src") ?? "";
    if (!hasAllowedYoutubeSource(src)) frame.remove();
  }

  for (const blockquote of document.body.querySelectorAll<HTMLElement>("blockquote[cite]")) {
    const cite = blockquote.getAttribute("cite") ?? "";
    if (!isSafePublishableUrl(cite, false)) blockquote.removeAttribute("cite");
  }
}

/**
 * Parser-backed allowlist for HTML that is leaving PostEditor as FINAL
 * MainSite content. This mirrors admin-app's current `sanitizePostHtml`
 * surface while using the DOMPurify dependency already shipped by Maestro.
 */
export function sanitizeFinalPostHtml(html: string): string {
  if (!html || typeof html !== "string") return "";
  const purified = DOMPurify.sanitize(html, {
    ALLOWED_TAGS: [...ALLOWED_TAGS],
    ALLOWED_ATTR: ALLOWED_ATTRIBUTES,
    ALLOW_ARIA_ATTR: false,
    ALLOW_DATA_ATTR: false,
    KEEP_CONTENT: true,
  });
  const parser = new DOMParser();
  const document = parser.parseFromString(String(purified), "text/html");
  enforceTagAttributeAllowlist(document);
  enforceUrlAndTransformRules(document);
  return document.body.innerHTML.trim();
}

/** Complete final-submit transform: parser allowlist first, link parity last. */
export function sanitizeFinalMainSiteHtml(html: string): string {
  return sanitizeMainSiteLinks(sanitizeFinalPostHtml(html));
}
