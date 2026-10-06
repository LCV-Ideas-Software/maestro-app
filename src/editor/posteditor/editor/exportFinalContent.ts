import { sanitizeFinalMainSiteHtml } from "./sanitizeFinalHtml";
import { normalizeSharedChatEvidence, type StoredSharedChatEvidence } from "./sharedChatImport";

export type FinalContentExportFormat = "markdown" | "html";
export type FinalContentProvenanceFormat = FinalContentExportFormat | "pdf";

export type FinalContentExportInput = {
  title: string;
  author: string;
  html: string;
  exportedAt?: string;
  evidence?: StoredSharedChatEvidence[];
};

export type ExportArtifact = {
  filename: string;
  mimeType: string;
  content: string;
};

export type FinalContentExport = {
  content: ExportArtifact;
  provenance: ExportArtifact;
};

const WINDOWS_RESERVED_FILENAME = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])$/i;
const PRINT_ROOT_ID = "maestro-final-content-print";
const PRINT_MEDIA_TIMEOUT_MS = 30_000;
const activePrintRequests = new WeakMap<Window, { cleanup: () => void; inCall: boolean }>();

function plainText(value: unknown, maxLength: number): string {
  if (typeof value !== "string") return "";
  return value.replace(/\s+/g, " ").trim().slice(0, maxLength);
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function escapeMarkdown(value: string): string {
  return value.replace(/([\\`~*_[\]{}()#+.!|<>-])/g, "\\$1");
}

function escapeCitationAuditText(value: string): string {
  return value.replace(/\s+/g, " ").replace(/([\\`~*_[\]{}#+!|<>])/g, "\\$1");
}

function markdownDestination(value: string): string {
  return value.replace(/>/g, "%3E");
}

function markdownQuotedTitle(value: string): string {
  let escaped = "";
  for (const character of value) {
    if (character === "\\" || character === '"') escaped += `\\${character}`;
    else if (character === "\r" || character === "\n") escaped += " ";
    else escaped += character;
  }
  return escaped;
}

function serializeChildren(element: Element, forCitationAudit = false): string {
  return [...element.childNodes]
    .map((child) => serializeMarkdownNode(child, forCitationAudit))
    .join("");
}

function serializeList(element: Element, ordered: boolean, forCitationAudit = false): string {
  const start = ordered ? Number(element.getAttribute("start")) || 1 : 1;
  return [...element.children]
    .filter((child) => child.tagName.toLowerCase() === "li")
    .map((item, index) => {
      const marker = ordered ? `${start + index}. ` : "- ";
      const body = serializeChildren(item, forCitationAudit).trim().replace(/\n+/g, "\n  ");
      return `${marker}${body}`;
    })
    .join("\n");
}

function serializeMarkdownNode(node: Node, forCitationAudit = false): string {
  if (node.nodeType === 3)
    return forCitationAudit
      ? escapeCitationAuditText(node.textContent ?? "")
      : escapeMarkdown(node.textContent ?? "");
  if (node.nodeType !== 1) return "";

  const element = node as Element;
  const tag = element.tagName.toLowerCase();
  const children = serializeChildren(element, forCitationAudit);

  if (/^h[1-6]$/.test(tag)) {
    return `${"#".repeat(Number(tag[1]))} ${children.trim()}\n\n`;
  }

  switch (tag) {
    case "p":
      return `${children.trim()}\n\n`;
    case "strong":
    case "b":
      return `**${children}**`;
    case "em":
    case "i":
      return `*${children}*`;
    case "s":
    case "del":
      if (forCitationAudit) return children;
      return children.trim() ? `~~${children}~~` : "";
    case "u":
    case "sub":
    case "sup":
      return forCitationAudit ? children : element.outerHTML;
    case "code": {
      if (forCitationAudit) return " ";
      const value = (element.textContent ?? "").replace(/\r\n?|\n/g, " ");
      if (!value) return "";
      const longestRun = (value.match(/`+/g) ?? []).reduce(
        (longest, run) => Math.max(longest, run.length),
        0,
      );
      const delimiter = "`".repeat(longestRun + 1);
      const padding = value.startsWith("`") || value.endsWith("`") ? " " : "";
      return `${delimiter}${padding}${value}${padding}${delimiter}`;
    }
    case "pre": {
      if (forCitationAudit) return " ";
      const value = element.textContent ?? "";
      const longestRun = (value.match(/`+/g) ?? []).reduce(
        (longest, run) => Math.max(longest, run.length),
        0,
      );
      const fence = "`".repeat(Math.max(3, longestRun + 1));
      return `\n${fence}\n${value}\n${fence}\n\n`;
    }
    case "blockquote":
      return `${children
        .trim()
        .split("\n")
        .map((line) => `> ${line}`)
        .join("\n")}\n\n`;
    case "ul":
      if (element.getAttribute("data-type") === "taskList" && !forCitationAudit)
        return `${element.outerHTML}\n\n`;
      return `${serializeList(element, false, forCitationAudit)}\n\n`;
    case "ol":
      return `${serializeList(element, true, forCitationAudit)}\n\n`;
    case "a": {
      const href = element.getAttribute("href");
      if (!href) return children;
      const title = element.getAttribute("title");
      const suffix = title ? ` "${markdownQuotedTitle(title)}"` : "";
      return `[${children}](<${markdownDestination(href)}>${suffix})`;
    }
    case "img": {
      const src = element.getAttribute("src");
      if (!src) return "";
      const alt = escapeMarkdown(element.getAttribute("alt") ?? "");
      const title = element.getAttribute("title");
      const suffix = title ? ` "${markdownQuotedTitle(title)}"` : "";
      return `![${alt}](<${markdownDestination(src)}>${suffix})`;
    }
    case "br":
      return "  \n";
    case "hr":
      return "\n---\n\n";
    case "tr":
      return forCitationAudit ? `${children.trimEnd()}\n` : children;
    case "td":
    case "th":
      return forCitationAudit ? `${children.trim()} ` : children;
    case "table":
    case "figure":
    case "iframe":
      return forCitationAudit ? `${children}\n\n` : `${element.outerHTML}\n\n`;
    case "div":
      return element.hasAttribute("data-youtube-video") && !forCitationAudit
        ? `${element.outerHTML}\n\n`
        : children;
    default:
      return children;
  }
}

function htmlToMarkdown(html: string, forCitationAudit = false): string {
  const document = new DOMParser().parseFromString(html, "text/html");
  const markdown = [...document.body.childNodes]
    .map((node) => serializeMarkdownNode(node, forCitationAudit))
    .join("")
    .replace(/\n{3,}/g, "\n\n");
  return (forCitationAudit ? markdown.replace(/(^|\n)[ \t]+/g, "$1") : markdown).trim();
}

export function htmlToCitationAuditMarkdown(html: string): string {
  return htmlToMarkdown(sanitizeFinalMainSiteHtml(html), true);
}

export function htmlToLinkAuditMarkdown(html: string): string {
  const sanitized = sanitizeFinalMainSiteHtml(html);
  const document = new DOMParser().parseFromString(sanitized, "text/html");
  const embeddedUrls = [
    ...document.body.querySelectorAll<HTMLIFrameElement | HTMLElement>(
      "iframe[src], blockquote[cite]",
    ),
  ]
    .map((element) => element.getAttribute(element.tagName === "IFRAME" ? "src" : "cite"))
    .filter((url): url is string => Boolean(url))
    .map((url) => `[fonte incorporada](<${markdownDestination(url)}>)`);
  return [htmlToMarkdown(sanitized, true), ...embeddedUrls].filter(Boolean).join("\n\n");
}

function exportableEvidence(evidence: StoredSharedChatEvidence[]): StoredSharedChatEvidence[] {
  return evidence.map((item) => ({
    provider: item.provider,
    ...normalizeSharedChatEvidence(item.provider, item),
  }));
}

function normalizedInput(input: FinalContentExportInput) {
  const title = plainText(input.title, 240) || "Artigo";
  const author = plainText(input.author, 240);
  const html = sanitizeFinalMainSiteHtml(input.html);
  if (!html || html === "<p></p>") throw new Error("Não há conteúdo final para exportar.");

  return {
    title,
    author,
    html,
    exportedAt: input.exportedAt ?? new Date().toISOString(),
    evidence: exportableEvidence(input.evidence ?? []),
  };
}

export function sanitizeExportFilename(title: string): string {
  const normalized = plainText(title, 240)
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 80)
    .replace(/-+$/g, "");
  const safe = normalized || "artigo";
  return WINDOWS_RESERVED_FILENAME.test(safe) ? `artigo-${safe}` : safe;
}

function buildProvenanceArtifact(
  input: ReturnType<typeof normalizedInput>,
  format: FinalContentProvenanceFormat,
  contentFilename: string,
): ExportArtifact {
  const base = sanitizeExportFilename(input.title);
  return {
    filename: `${base}.provenance.json`,
    mimeType: "application/json;charset=utf-8",
    content: `${JSON.stringify(
      {
        schema_version: "maestro.export-provenance.v1",
        exported_at: input.exportedAt,
        format,
        document: {
          title: input.title,
          author: input.author,
          filename: contentFilename,
        },
        evidence: input.evidence,
      },
      null,
      2,
    )}\n`,
  };
}

export function buildFinalContentExport(
  rawInput: FinalContentExportInput,
  format: FinalContentExportFormat,
): FinalContentExport {
  const input = normalizedInput(rawInput);
  const base = sanitizeExportFilename(input.title);
  const content: ExportArtifact =
    format === "html"
      ? {
          filename: `${base}.mainsite.html`,
          mimeType: "text/html;charset=utf-8",
          content: input.html,
        }
      : {
          filename: `${base}.md`,
          mimeType: "text/markdown;charset=utf-8",
          content: `# ${escapeMarkdown(input.title)}${
            input.author ? `\n\n> Autoria: ${escapeMarkdown(input.author)}` : ""
          }\n\n${htmlToMarkdown(input.html)}\n`,
        };

  return {
    content,
    provenance: buildProvenanceArtifact(input, format, content.filename),
  };
}

export function buildPdfProvenanceExport(rawInput: FinalContentExportInput): ExportArtifact {
  const input = normalizedInput(rawInput);
  const filename = `${sanitizeExportFilename(input.title)}.pdf`;
  return buildProvenanceArtifact(input, "pdf", filename);
}

function printDocumentStyles(selector: string): string {
  return `
    @page { margin: 2cm; }
    ${selector} { color: #111; font-family: Georgia, "Times New Roman", serif; line-height: 1.55; margin: 0 auto; max-width: 48rem; }
    ${selector} > header { border-bottom: 1px solid #bbb; margin-bottom: 2rem; padding-bottom: 1rem; }
    ${selector} h1 { line-height: 1.2; }
    ${selector} img, ${selector} iframe { height: auto; max-width: 100%; }
    ${selector} table { border-collapse: collapse; width: 100%; }
    ${selector} td, ${selector} th { border: 1px solid #999; padding: .4rem; }
    ${selector} pre { overflow-wrap: anywhere; white-space: pre-wrap; }
  `;
}

export function buildPrintDocument(rawInput: FinalContentExportInput): string {
  const input = normalizedInput(rawInput);
  const title = escapeHtml(input.title);
  const author = escapeHtml(input.author);
  return `<!doctype html>
<html lang="pt-BR">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="maestro-export" content="pdf-print">
  <title>${title}</title>
  <style>${printDocumentStyles("body")}</style>
</head>
<body>
  <header><h1>${title}</h1>${author ? `<p>Autoria: ${author}</p>` : ""}</header>
  <article>${input.html}</article>
</body>
</html>`;
}

export function downloadExportArtifact(
  artifact: ExportArtifact,
  ownerDocument: Document = document,
): void {
  const blob = new Blob([artifact.content], { type: artifact.mimeType });
  const url = URL.createObjectURL(blob);
  const anchor = ownerDocument.createElement("a");
  anchor.href = url;
  anchor.download = artifact.filename;
  anchor.hidden = true;
  ownerDocument.body.append(anchor);
  anchor.click();
  anchor.remove();
  queueMicrotask(() => URL.revokeObjectURL(url));
}

function preparePrintMedia(projection: HTMLElement, ownerWindow: Window) {
  const media = [
    ...projection.querySelectorAll<HTMLImageElement | HTMLIFrameElement>("img, iframe"),
  ];
  let resolveReady: () => void;
  let rejectReady: (error: Error) => void;
  const ready = new Promise<void>((resolve, reject) => {
    resolveReady = resolve;
    rejectReady = reject;
  });
  const removeListeners: (() => void)[] = [];
  const checkCachedImages: (() => void)[] = [];
  let remaining = media.length;
  let finished = false;
  let timer: number | undefined;
  const finish = (error?: Error) => {
    if (finished) return;
    finished = true;
    for (const remove of removeListeners) remove();
    if (timer !== undefined) ownerWindow.clearTimeout(timer);
    if (error) rejectReady(error);
    else resolveReady();
  };
  const settled = () => {
    if (finished) return;
    remaining -= 1;
    if (remaining === 0) finish();
  };
  for (const element of media) {
    // The projection stays hidden on screen. Native eager loading avoids
    // lazy resources waiting for a viewport that only exists during print.
    element.loading = "eager";
    let handled = false;
    const remove = () => {
      element.removeEventListener("load", loaded);
      element.removeEventListener("error", failed);
    };
    const failed = () => {
      if (handled) return;
      handled = true;
      remove();
      settled();
    };
    const loaded = () => {
      if (handled) return;
      handled = true;
      remove();
      if (element.tagName === "IMG" && typeof (element as HTMLImageElement).decode === "function") {
        try {
          (element as HTMLImageElement).decode().then(settled, settled);
        } catch {
          settled();
        }
      } else {
        settled();
      }
    };
    removeListeners.push(remove);
    element.addEventListener("load", loaded);
    element.addEventListener("error", failed);
    if (element.tagName === "IMG") {
      checkCachedImages.push(() => {
        if ((element as HTMLImageElement).complete) loaded();
      });
    }
  }
  if (remaining === 0) finish();
  else {
    timer = ownerWindow.setTimeout(
      () =>
        finish(
          new Error(
            "As mídias não terminaram de carregar. Verifique a conexão e tente exportar o PDF novamente.",
          ),
        ),
      PRINT_MEDIA_TIMEOUT_MS,
    );
  }
  return {
    ready,
    checkCachedImages: () => {
      for (const check of checkCachedImages) check();
    },
    cancel: () => finish(new Error("A preparação da impressão foi cancelada.")),
  };
}

export async function openFinalContentPrintDialog(
  input: FinalContentExportInput,
  ownerWindow: Window = window,
): Promise<void> {
  const previous = activePrintRequests.get(ownerWindow);
  if (previous?.inCall) {
    throw new Error("Uma solicitação de impressão já está em andamento.");
  }
  // Ignored native print requests may emit no afterprint event. Retire only
  // our previous projection before taking the next document snapshot.
  previous?.cleanup();
  const ownerDocument = ownerWindow.document;
  if (ownerDocument.getElementById(PRINT_ROOT_ID)) {
    throw new Error("A área de impressão já está ocupada.");
  }
  const printable = new DOMParser().parseFromString(buildPrintDocument(input), "text/html");
  const projection = ownerDocument.createElement("section");
  projection.id = PRINT_ROOT_ID;
  projection.dataset.maestroExport = "pdf-print";
  projection.style.display = "none";
  for (const child of printable.body.childNodes) {
    projection.append(ownerDocument.importNode(child, true));
  }
  const style = ownerDocument.createElement("style");
  style.dataset.maestroExport = "pdf-print";
  style.media = "print";
  style.textContent = `
    html, body { height: auto !important; min-height: 0 !important; max-height: none !important; overflow: visible !important; }
    body { display: block !important; margin: 0 !important; background: white !important; }
    body > :not(#${PRINT_ROOT_ID}) { display: none !important; }
    #${PRINT_ROOT_ID} { display: block !important; height: auto !important; overflow: visible !important; }
    ${printDocumentStyles(`#${PRINT_ROOT_ID}`)}
  `;
  const originalTitle = ownerDocument.title;
  let mediaPreparation: ReturnType<typeof preparePrintMedia> | undefined;
  let cleaned = false;
  const cleanup = () => {
    if (cleaned) return;
    cleaned = true;
    ownerWindow.removeEventListener("afterprint", cleanup);
    ownerWindow.removeEventListener("pagehide", cleanup);
    mediaPreparation?.cancel();
    projection.remove();
    style.remove();
    ownerDocument.title = originalTitle;
    if (activePrintRequests.get(ownerWindow) === request) activePrintRequests.delete(ownerWindow);
  };
  const request = { cleanup, inCall: true };
  activePrintRequests.set(ownerWindow, request);
  try {
    ownerWindow.addEventListener("afterprint", cleanup, { once: true });
    ownerWindow.addEventListener("pagehide", cleanup, { once: true });
    ownerDocument.head.append(style);
    // Subscribe before insertion so even an immediately loaded frame is observed.
    mediaPreparation = preparePrintMedia(projection, ownerWindow);
    ownerDocument.body.append(projection);
    ownerDocument.title = printable.title;
    mediaPreparation.checkCachedImages();
    await mediaPreparation.ready;
    if (cleaned || activePrintRequests.get(ownerWindow) !== request) {
      throw new Error("A preparação da impressão foi cancelada.");
    }
    // WebView2 does not create browser popups by default. Print its existing
    // top-level document; print media exposes only the sanitized article.
    ownerWindow.print();
  } catch (error) {
    cleanup();
    throw error;
  } finally {
    // Native print may return before its dialog closes. afterprint owns DOM
    // cleanup, including cancellation; return does not prove a PDF was saved.
    request.inCall = false;
  }
}
