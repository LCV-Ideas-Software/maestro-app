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

// Reject hrefs whose scheme can execute script or smuggle content. TipTap's
// Link extension already enforces a protocol allowlist, but the app's own link
// helpers (AutoTargetBlankLink, sanitizeLinksTargetBlank) must not depend on
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
  // Mirror the safe subset of TipTap's own protocol allowlist so legitimate
  // editorial links (incl. ftp) are not dropped; only script-capable schemes
  // (javascript/data/vbscript/file/blob/…) fall outside this set.
  return !["http", "https", "ftp", "ftps", "mailto", "tel"].includes(scheme);
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
