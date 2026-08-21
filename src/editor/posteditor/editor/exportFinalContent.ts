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
  return value.replace(/([\\`*_[\]{}()#+.!|<>-])/g, "\\$1");
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

function serializeChildren(element: Element): string {
  return [...element.childNodes].map((child) => serializeMarkdownNode(child)).join("");
}

function serializeList(element: Element, ordered: boolean): string {
  const start = ordered ? Number(element.getAttribute("start")) || 1 : 1;
  return [...element.children]
    .filter((child) => child.tagName.toLowerCase() === "li")
    .map((item, index) => {
      const marker = ordered ? `${start + index}. ` : "- ";
      const body = serializeChildren(item).trim().replace(/\n+/g, "\n  ");
      return `${marker}${body}`;
    })
    .join("\n");
}

function serializeMarkdownNode(node: Node): string {
  if (node.nodeType === 3) return escapeMarkdown(node.textContent ?? "");
  if (node.nodeType !== 1) return "";

  const element = node as Element;
  const tag = element.tagName.toLowerCase();
  const children = serializeChildren(element);

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
      return `~~${children}~~`;
    case "u":
    case "sub":
    case "sup":
      return element.outerHTML;
    case "code": {
      const value = element.textContent ?? "";
      const delimiter = value.includes("`") ? "``" : "`";
      return `${delimiter}${value}${delimiter}`;
    }
    case "pre":
      return `\n\`\`\`\n${element.textContent ?? ""}\n\`\`\`\n\n`;
    case "blockquote":
      return `${children
        .trim()
        .split("\n")
        .map((line) => `> ${line}`)
        .join("\n")}\n\n`;
    case "ul":
      if (element.getAttribute("data-type") === "taskList") return `${element.outerHTML}\n\n`;
      return `${serializeList(element, false)}\n\n`;
    case "ol":
      return `${serializeList(element, true)}\n\n`;
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
    case "table":
    case "figure":
    case "iframe":
      return `${element.outerHTML}\n\n`;
    case "div":
      return element.hasAttribute("data-youtube-video") ? `${element.outerHTML}\n\n` : children;
    default:
      return children;
  }
}

function htmlToMarkdown(html: string): string {
  const document = new DOMParser().parseFromString(html, "text/html");
  return [...document.body.childNodes]
    .map((node) => serializeMarkdownNode(node))
    .join("")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
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
  <style>
    @page { margin: 2cm; }
    body { color: #111; font-family: Georgia, "Times New Roman", serif; line-height: 1.55; margin: 0 auto; max-width: 48rem; }
    header { border-bottom: 1px solid #bbb; margin-bottom: 2rem; padding-bottom: 1rem; }
    h1 { line-height: 1.2; }
    img, iframe { height: auto; max-width: 100%; }
    table { border-collapse: collapse; width: 100%; }
    td, th { border: 1px solid #999; padding: .4rem; }
    pre { overflow-wrap: anywhere; white-space: pre-wrap; }
  </style>
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

export function openFinalContentPrintDialog(
  input: FinalContentExportInput,
  ownerWindow: Window = window,
): void {
  const documentBlob = new Blob([buildPrintDocument(input)], { type: "text/html;charset=utf-8" });
  const url = URL.createObjectURL(documentBlob);
  const printWindow = ownerWindow.open(url, "_blank");
  if (!printWindow) {
    URL.revokeObjectURL(url);
    throw new Error("O sistema bloqueou a janela de impressão.");
  }
  printWindow.opener = null;
  printWindow.addEventListener(
    "load",
    () => {
      URL.revokeObjectURL(url);
      printWindow.focus();
      printWindow.print();
    },
    { once: true },
  );
}
