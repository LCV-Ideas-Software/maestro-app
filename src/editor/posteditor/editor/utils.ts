export const clamp = (value: number, min: number, max: number): number =>
  Math.max(min, Math.min(max, value));

export const formatImageUrl = (url: string): string => {
  if (!url) return "";
  const driveRegex = /(?:file\/d\/|open\?id=|uc\?id=)([a-zA-Z0-9_-]+)/;
  const match = url.match(driveRegex);
  if (match?.[1]) return `https://drive.google.com/uc?export=view&id=${match[1]}`;
  return url;
};

export const isYoutubeUrl = (url: string): boolean => /(?:youtube\.com|youtu\.be)\//i.test(url);

function hasUrlControlCharacters(value: string): boolean {
  return [...value].some((character) => {
    const codePoint = character.codePointAt(0) ?? 0;
    return codePoint <= 31 || (codePoint >= 127 && codePoint <= 159);
  });
}

/**
 * Applies the mechanical MainSite link rules before HTML is persisted.
 *
 * This helper deliberately does not replace a URL or decide whether it
 * supports an editorial claim. Those decisions belong to Link Integrity
 * review. It only removes unsafe hrefs and normalizes safe link attributes.
 */
export function sanitizeMainSiteLinks(html: string): string {
  if (!html) return html;

  const parser = new DOMParser();
  const doc = parser.parseFromString(html, "text/html");
  const anchors = doc.querySelectorAll<HTMLAnchorElement>("a[href]");
  let changed = false;

  for (const anchor of anchors) {
    const originalHref = anchor.getAttribute("href") ?? "";
    const href = originalHref.trim();

    if (!href || hasUrlControlCharacters(originalHref) || hasUnsafeUrlScheme(href)) {
      anchor.removeAttribute("href");
      anchor.removeAttribute("target");
      anchor.removeAttribute("rel");
      changed = true;
      continue;
    }

    if (href !== originalHref) {
      anchor.setAttribute("href", href);
      changed = true;
    }

    // YouTube links retain PostEditor/PostReader behavior. They are handled by
    // the dedicated YouTube extension and must not be rewritten as ordinary
    // external links.
    if (isYoutubeUrl(href)) continue;

    // Exact PostEditor parity: every safe non-YouTube link opens in a new tab,
    // including relative, fragment and mailto links. The final parser-backed
    // sanitizer separately enforces the publishable protocol allowlist.
    if (anchor.getAttribute("target") !== "_blank") {
      anchor.setAttribute("target", "_blank");
      changed = true;
    }
    if (anchor.getAttribute("rel") !== "noopener noreferrer") {
      anchor.setAttribute("rel", "noopener noreferrer");
      changed = true;
    }
  }

  return changed ? doc.body.innerHTML : html;
}

// Reject hrefs whose scheme can execute script or smuggle content. TipTap's
// Link extension already enforces a protocol allowlist, but the app's own link
// helpers (AutoTargetBlankLink, sanitizeMainSiteLinks) must not depend on
// that upstream default — this is the independent backstop. See audit S4.
// Relative, anchor and scheme-relative ("//host") hrefs have no scheme and are
// treated as safe; only an explicit dangerous scheme is rejected.
export const hasUnsafeUrlScheme = (url: string): boolean => {
  // Drop whitespace browsers ignore inside the scheme ("java\tscript:") before
  // matching, then require an explicit scheme to be on the safe allowlist.
  const normalized = url.replace(/\s+/g, "").toLowerCase();
  const schemeMatch = normalized.match(/^([a-z][a-z0-9+.-]*):/);
  if (!schemeMatch) return false;
  const scheme = schemeMatch[1] ?? "";
  // Keep the publishable protocol set identical to the Link Integrity Engine.
  // A protocol that the release gate cannot inventory must not survive the
  // MainSite sanitizer merely because a browser knows how to open it.
  return !["http", "https", "mailto"].includes(scheme);
};

export function migrateLegacyCaptions(html: string): string {
  if (!html) return html;

  const normalizeCaption = (caption: string) => {
    // Loop até estabilizar para resistir a padrões aninhados (`<a<b>>`) onde
    // um único pass deixaria caracteres residuais.
    let prev = "";
    let out = caption;
    while (prev !== out) {
      prev = out;
      out = out.replace(/<[^>]+>/g, "");
    }
    return out.replace(/\s+/g, " ").trim();
  };

  const wrappedImagePattern =
    /<p[^>]*>\s*(<img\b[^>]*>)\s*<\/p>\s*<p[^>]*text-align\s*:\s*center[^>]*>\s*(?:<em>|<i>)\s*([\s\S]*?)\s*(?:<\/em>|<\/i>)\s*<\/p>/gi;
  const plainImagePattern =
    /(<img\b[^>]*>)\s*<p[^>]*text-align\s*:\s*center[^>]*>\s*(?:<em>|<i>)\s*([\s\S]*?)\s*(?:<\/em>|<\/i>)\s*<\/p>/gi;

  let migrated = html.replace(wrappedImagePattern, (_m, imgTag: string, captionRaw: string) => {
    const caption = normalizeCaption(captionRaw);
    if (!caption) return `<figure class="tiptap-figure">${imgTag}</figure>`;
    return `<figure class="tiptap-figure">${imgTag}<figcaption>${caption}</figcaption></figure>`;
  });

  migrated = migrated.replace(plainImagePattern, (_m, imgTag: string, captionRaw: string) => {
    const caption = normalizeCaption(captionRaw);
    if (!caption) return `<figure class="tiptap-figure">${imgTag}</figure>`;
    return `<figure class="tiptap-figure">${imgTag}<figcaption>${caption}</figcaption></figure>`;
  });

  return migrated;
}
