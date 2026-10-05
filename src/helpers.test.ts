import { describe, expect, it } from "vitest";
import {
  agentResultRank,
  attachmentDeliveryPlan,
  countAgentRounds,
  latestAgentResults,
  latestProtocolGateItems,
  providerSupportsNativeAttachment,
} from "./helpers";
import type { EditorialAgentResult } from "./types";

function result(round: string, status: string): EditorialAgentResult {
  return {
    name: "Claude",
    cli: "claude",
    tone: status === "READY" ? "ok" : "warn",
    status,
    duration_ms: 10,
    exit_code: 0,
    role: "review",
    output_path: `agent-runs/round-${round}-claude-review.md`,
  };
}

describe("editorial round display", () => {
  it("ranks and counts the minimum-width native round format after round 999", () => {
    const previous = result("999", "READY");
    const latest = result("1000", "NOT_READY");
    expect(agentResultRank(latest)).toBeGreaterThan(agentResultRank(previous));
    expect(latestAgentResults([previous, latest])).toEqual([latest]);
    expect(countAgentRounds([previous, latest])).toBe(2);
  });

  it("retains padded first-round behavior and ignores unrelated artifacts", () => {
    expect(agentResultRank(result("001", "READY"))).toBe(13);
    expect(
      countAgentRounds([
        result("001", "READY"),
        { ...result("001", "READY"), output_path: "draft.md" },
      ]),
    ).toBe(1);
  });

  it.each(["READY", "NOT_READY", "API_HTTP_429"])(
    "shows actual phase without inventing protocol reading telemetry for %s",
    (status) => {
      const [gate] = latestProtocolGateItems([result("001", status)]);
      expect(gate?.progress).toBeNull();
      expect(gate?.status).toContain("Revisao:");
      expect(gate?.status).not.toContain("Protocolo lido");
    },
  );
});

describe("native attachment prediction", () => {
  it.each(["image/gif", "audio/midi", "video/unknown"])(
    "routes unsupported Gemini media %s to the evidence manifest",
    (media_type) => {
      const attachment = { name: "media", media_type, size_bytes: 10, data_base64: "eA==" };
      expect(providerSupportsNativeAttachment("gemini", attachment)).toBe(false);
      expect(attachmentDeliveryPlan(attachment, ["gemini"]).manifestProviders).toEqual(["gemini"]);
    },
  );

  it.each(["image/heic", "image/heif", "audio/opus", "video/3gpp"])(
    "keeps the native Gemini media format %s",
    (media_type) => {
      expect(
        providerSupportsNativeAttachment("gemini", {
          name: "media",
          media_type,
          size_bytes: 10,
          data_base64: "eA==",
        }),
      ).toBe(true);
    },
  );

  it("preserves GIF support for OpenAI and Anthropic", () => {
    const attachment = {
      name: "image.gif",
      media_type: "image/gif",
      size_bytes: 10,
      data_base64: "eA==",
    };
    expect(providerSupportsNativeAttachment("openai", attachment)).toBe(true);
    expect(providerSupportsNativeAttachment("anthropic", attachment)).toBe(true);
  });

  it.each([
    ["document.docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"],
    ["sheet.xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"],
    ["slides.pptx", "application/vnd.openxmlformats-officedocument.presentationml.presentation"],
    ["document.odt", "application/vnd.oasis.opendocument.text"],
    ["notes.txt", "application/octet-stream"],
    ["document.pdf", "application/octet-stream"],
  ])("does not promise unsupported Gemini blobs or infer MIME from %s", (name, media_type) => {
    const attachment = { name, media_type, size_bytes: 10, data_base64: "eA==" };
    expect(providerSupportsNativeAttachment("gemini", attachment)).toBe(false);
    expect(providerSupportsNativeAttachment("openai", attachment)).toBe(true);
    const plan = attachmentDeliveryPlan(attachment, ["openai", "gemini"]);
    expect(plan.nativeProviders).toEqual(["openai"]);
    expect(plan.manifestProviders).toEqual(["gemini"]);
  });

  it.each([
    "text/markdown",
    "application/pdf",
    "application/json",
    "application/rtf",
    "application/x-typescript",
  ])("keeps supported Gemini MIME %s", (media_type) => {
    expect(
      providerSupportsNativeAttachment("gemini", {
        name: "document",
        media_type,
        size_bytes: 10,
        data_base64: "eA==",
      }),
    ).toBe(true);
  });

  it.each(["image/png", "image/jpeg", " IMAGE/JPG "])(
    "predicts native Grok input for the official image MIME %s",
    (media_type) => {
      const attachment = { name: "image", media_type, size_bytes: 10, data_base64: "eA==" };
      expect(providerSupportsNativeAttachment("grok", attachment)).toBe(true);
      const plan = attachmentDeliveryPlan(attachment, ["grok", "deepseek", "perplexity"]);
      expect(plan.nativeProviders).toEqual(["grok"]);
      expect(plan.manifestProviders).toEqual(["deepseek", "perplexity"]);
      expect(plan.fallbackReason).toBe("API text-only");
    },
  );

  it("accepts the inclusive Grok raw-image boundary and forecasts fallback above it", () => {
    const attachment = {
      name: "image.png",
      media_type: "image/png",
      size_bytes: 20 * 1024 * 1024,
      data_base64: "eA==",
    };
    expect(attachmentDeliveryPlan(attachment, ["grok"]).nativeProviders).toEqual(["grok"]);
    const oversized = attachmentDeliveryPlan(
      { ...attachment, size_bytes: attachment.size_bytes + 1 },
      ["grok"],
    );
    expect(oversized.nativeProviders).toEqual([]);
    expect(oversized.manifestProviders).toEqual(["grok"]);
    expect(oversized.fallbackReason).toContain("excede envio nativo");
  });

  it.each([
    ["image.gif", "image/gif"],
    ["image.webp", "image/webp"],
    ["document.pdf", "application/pdf"],
    ["document.docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"],
    ["image.png", "application/octet-stream"],
  ])("does not infer unsupported Grok native content from %s", (name, media_type) => {
    const attachment = { name, media_type, size_bytes: 10, data_base64: "eA==" };
    const plan = attachmentDeliveryPlan(attachment, ["grok"]);
    expect(providerSupportsNativeAttachment("grok", attachment)).toBe(false);
    expect(plan.nativeProviders).toEqual([]);
    expect(plan.manifestProviders).toEqual(["grok"]);
    expect(plan.fallbackReason).toBe("tipo sem suporte nativo nos peers API ativos");
  });

  it("keeps supported image transport distinct from text-only peers in a full roster", () => {
    const attachment = {
      name: "image.jpeg",
      media_type: "image/jpeg",
      size_bytes: 10,
      data_base64: "eA==",
    };
    const plan = attachmentDeliveryPlan(attachment, [
      "openai",
      "anthropic",
      "gemini",
      "deepseek",
      "grok",
      "perplexity",
    ]);
    expect(plan.nativeProviders).toEqual(["openai", "anthropic", "gemini", "grok"]);
    expect(plan.manifestProviders).toEqual(["deepseek", "perplexity"]);
    expect(plan.fallbackReason).toBe("API text-only");
  });
});
