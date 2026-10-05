import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  fetchWebEvidence,
  getWebEvidence,
  importOperatorEvidence,
  listWebEvidence,
} from "../../services/evidence";
import type { WebEvidenceRecord } from "../../types";
import { EvidenceScreen } from "./EvidenceScreen";

vi.mock("../../services/evidence", () => ({
  fetchWebEvidence: vi.fn(),
  getWebEvidence: vi.fn(),
  importOperatorEvidence: vi.fn(),
  listWebEvidence: vi.fn(),
  openWebEvidenceInDefaultBrowser: vi.fn(),
  replayWebEvidence: vi.fn(),
  resumeWebEvidenceInteraction: vi.fn(),
  searchWebEvidence: vi.fn(),
  startRenderedWebEvidence: vi.fn(),
}));
vi.mock("../../services/nativeEvents", () => ({
  listenToWebEvidenceProgress: vi.fn(async () => vi.fn()),
}));
vi.mock("./LinkIntegrityPanel", () => ({ LinkIntegrityPanel: () => null }));

const storedRecord: WebEvidenceRecord = {
  id: "stored-evidence",
  schema_version: "web_evidence.v1",
  state: "ready",
  url: "https://example.com/stored",
  method: "GET",
  access_mode: "http_fetch",
  status: 200,
  final_url: "https://example.com/stored",
  title: "Stored evidence",
  content_type: "text/html",
  sha256: "a".repeat(64),
  retrieved_at: "2026-10-05T00:00:00Z",
  expires_at: null,
  cache_ttl: "24h",
  cache_state: "fresh",
  robots_state: "allowed",
  copyright_state: "public",
  interaction_state: "none",
  human_resolved: false,
  byte_count: 100,
  duration_ms: 1,
  redirect_chain: [],
  curl_command: null,
  provider: null,
  query: null,
  artifact_name: null,
  notes: [],
  created_at: "2026-10-05T00:00:00Z",
  updated_at: "2026-10-05T00:00:00Z",
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, fail) => {
    resolve = accept;
    reject = fail;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(listWebEvidence).mockResolvedValue({
    items: [storedRecord],
    next_cursor: null,
    total: 1,
  });
});
afterEach(() => cleanup());

async function renderEvidence() {
  render(
    <EvidenceScreen
      evidenceRows={[]}
      linkAuditRows={[]}
      citationAuditResult={null}
      isAuditing={false}
      onAudit={vi.fn()}
    />,
  );
  await waitFor(() => expect(screen.getByRole("button", { name: "Coletar HTTP" })).toBeEnabled());
  return within(screen.getByLabelText("Evidências armazenadas")).getByRole("button", {
    name: /Stored evidence/,
  });
}

function tryConcurrentReads(recordButton: HTMLElement) {
  fireEvent.click(recordButton);
  fireEvent.keyDown(screen.getByRole("searchbox", { name: "Filtrar inventário de evidências" }), {
    key: "Enter",
  });
  expect(getWebEvidence).not.toHaveBeenCalled();
  expect(listWebEvidence).toHaveBeenCalledTimes(1);
  expect(recordButton).toBeDisabled();
  expect(screen.getByRole("button", { name: "Coletar HTTP" })).toBeDisabled();
}

describe("EvidenceScreen busy ownership", () => {
  it("keeps a pending collection busy when a detail or Enter-triggered inventory read is attempted", async () => {
    const pending = deferred<WebEvidenceRecord>();
    vi.mocked(fetchWebEvidence).mockReturnValueOnce(pending.promise);
    const recordButton = await renderEvidence();
    fireEvent.change(screen.getByLabelText("URL pública"), {
      target: { value: "https://example.com/new" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Coletar HTTP" }));
    expect(fetchWebEvidence).toHaveBeenCalledOnce();
    tryConcurrentReads(recordButton);
    await act(async () =>
      pending.resolve({ ...storedRecord, id: "collected-evidence", title: "Collected evidence" }),
    );
    expect(screen.getByRole("button", { name: "Coletar HTTP" })).toBeEnabled();
  });

  it("keeps a pending artifact import busy until that import settles", async () => {
    const pending = deferred<WebEvidenceRecord>();
    vi.mocked(importOperatorEvidence).mockReturnValueOnce(pending.promise);
    const recordButton = await renderEvidence();
    const file = new File(["operator capture"], "capture.txt", { type: "text/plain" });
    Object.defineProperty(file, "arrayBuffer", {
      value: async () => new TextEncoder().encode("operator capture").buffer,
    });
    const input = document.getElementById("evidence-capture-file");
    if (!input) throw new Error("Capture input is missing");
    fireEvent.change(input, { target: { files: [file] } });
    fireEvent.click(screen.getByRole("button", { name: "Importar com proveniência" }));
    await waitFor(() => expect(importOperatorEvidence).toHaveBeenCalledOnce());
    tryConcurrentReads(recordButton);
    expect(input).toBeDisabled();
    expect(screen.getByLabelText("URL de origem")).toBeDisabled();
    expect(screen.getByLabelText("Nota de proveniência")).toBeDisabled();
    await act(async () => pending.reject(new Error("import failed")));
    expect(screen.getByRole("button", { name: "Importar com proveniência" })).toBeEnabled();
    expect(input).toBeEnabled();
    expect(screen.getByRole("button", { name: "Coletar HTTP" })).toBeEnabled();
  });

  it("locks mutations during a detail reload and restores controls after a rejected read", async () => {
    const pending = deferred<WebEvidenceRecord>();
    vi.mocked(getWebEvidence).mockReturnValueOnce(pending.promise);
    const recordButton = await renderEvidence();
    fireEvent.click(recordButton);
    expect(getWebEvidence).toHaveBeenCalledWith("stored-evidence");
    expect(screen.getByRole("button", { name: "Coletar HTTP" })).toBeDisabled();
    fireEvent.click(recordButton);
    expect(getWebEvidence).toHaveBeenCalledOnce();
    await act(async () => pending.reject(new Error("read failed")));
    expect(recordButton).toBeEnabled();
    expect(screen.getByRole("button", { name: "Coletar HTTP" })).toBeEnabled();
    expect(
      screen.getByText("Não foi possível atualizar os detalhes desta evidência."),
    ).toBeInTheDocument();
  });
});
