import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AiProviderConfig, BootstrapConfig } from "../types";
import {
  listResumableSessions,
  resumeEditorialSession,
  runEditorialSession,
  stopEditorialSession,
} from "./editorial";
import {
  auditLinks,
  fetchWebEvidence,
  getWebEvidence,
  importOperatorEvidence,
  listWebEvidence,
  openWebEvidenceInDefaultBrowser,
  replayWebEvidence,
  resumeWebEvidenceInteraction,
  searchWebEvidence,
  startRenderedWebEvidence,
} from "./evidence";
import {
  listenToNativeLogs,
  listenToRuntimeBootstrapProgress,
  listenToWebEvidenceProgress,
} from "./nativeEvents";
import {
  controlRuntimeBootstrapAction,
  createRuntimeBootstrapPlan,
  dependencyPreflight,
  executeRuntimeBootstrapAction,
  openDataFile,
  readBootstrapConfig,
  readCloudflareEnvSnapshot,
  writeBootstrapConfig,
} from "./runtime";
import {
  probeAiProviderCredentials,
  probeCloudflareCredentials,
  readAiProviderConfig,
  writeAiProviderConfig,
} from "./settings";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

describe("Tauri service facades", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
    listenMock.mockReset();
  });

  it("keeps runtime and evidence command names and payloads stable", async () => {
    const bootstrap = { schema_version: 1 } as BootstrapConfig;

    await dependencyPreflight();
    await openDataFile("data/sessions/example/ata.md");
    await writeBootstrapConfig(bootstrap);
    await auditLinks("https://example.com");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "dependency_preflight");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "open_data_file", {
      path: "data/sessions/example/ata.md",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "write_bootstrap_config", {
      config: bootstrap,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(4, "audit_links", {
      request: { text: "https://example.com" },
    });
  });

  it("keeps all read command names stable", async () => {
    await readBootstrapConfig();
    await readCloudflareEnvSnapshot();
    await readAiProviderConfig();
    await listResumableSessions();

    expect(invokeMock).toHaveBeenNthCalledWith(1, "read_bootstrap_config");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "cloudflare_env_snapshot");
    expect(invokeMock).toHaveBeenNthCalledWith(3, "read_ai_provider_config");
    expect(invokeMock).toHaveBeenNthCalledWith(4, "list_resumable_sessions");
  });

  it("keeps runtime bootstrap authorization payloads fail-closed", async () => {
    await createRuntimeBootstrapPlan();
    await executeRuntimeBootstrapAction("install-codex", "sha256-plan", true);
    await controlRuntimeBootstrapAction("install-codex", "sha256-plan", "defer");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "runtime_bootstrap_plan");
    expect(invokeMock).toHaveBeenNthCalledWith(2, "execute_runtime_bootstrap_action", {
      actionId: "install-codex",
      planHash: "sha256-plan",
      approved: true,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "runtime_bootstrap_action_control", {
      actionId: "install-codex",
      planHash: "sha256-plan",
      disposition: "defer",
    });
  });

  it("keeps web evidence commands behind typed request envelopes", async () => {
    const fetchRequest = {
      url: "https://example.com/source",
      method: "GET" as const,
      force_revalidate: true,
    };
    const searchRequest = { query: "source title", provider: "crossref", limit: 10 };
    const importRequest = {
      url: "https://example.com/source",
      name: "source.md",
      media_type: "text/markdown",
      data_base64: "IyBTb3VyY2U=",
      notes: ["captura autorizada"],
    };

    await listWebEvidence({ states: ["stale"], limit: 50, cursor: null });
    await fetchWebEvidence(fetchRequest);
    await replayWebEvidence("evidence-1");
    await searchWebEvidence(searchRequest);
    await startRenderedWebEvidence("https://example.com/rendered");
    await openWebEvidenceInDefaultBrowser("https://example.com/manual");
    await importOperatorEvidence(importRequest);
    await resumeWebEvidenceInteraction("evidence-1", true);
    await getWebEvidence("evidence-1");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "list_web_evidence", {
      request: { states: ["stale"], limit: 50, cursor: null },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "fetch_web_evidence", {
      request: fetchRequest,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "replay_web_evidence", {
      request: { evidence_id: "evidence-1" },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(4, "search_web_evidence", {
      request: searchRequest,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(5, "start_rendered_web_evidence", {
      request: { url: "https://example.com/rendered" },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(6, "open_web_evidence_in_default_browser", {
      request: { url: "https://example.com/manual" },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(7, "import_operator_evidence", {
      request: importRequest,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(8, "resume_web_evidence_interaction", {
      request: { evidence_id: "evidence-1", confirmed: true },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(9, "get_web_evidence", {
      evidenceId: "evidence-1",
    });
  });

  it("keeps the native log event boundary stable", async () => {
    const payload = { category: "session.agent.started", context: { run_id: "run-1" } };
    const handler = vi.fn();
    const unlisten = vi.fn();
    listenMock.mockImplementation(async (eventName, callback) => {
      expect(eventName).toBe("maestro-log-event");
      callback({ payload } as never);
      return unlisten;
    });

    const registeredUnlisten = await listenToNativeLogs(handler);

    expect(handler).toHaveBeenCalledOnce();
    expect(handler).toHaveBeenCalledWith(payload);
    expect(registeredUnlisten).toBe(unlisten);
  });

  it("keeps runtime bootstrap progress behind its typed event boundary", async () => {
    const payload = {
      action_id: "install-codex",
      plan_hash: "sha256-plan",
      phase: "running",
      message: "Executando acao autorizada",
      at: "2026-08-21T12:00:00Z",
    };
    const handler = vi.fn();
    const unlisten = vi.fn();
    listenMock.mockImplementation(async (eventName, callback) => {
      expect(eventName).toBe("runtime-bootstrap-progress");
      callback({ payload } as never);
      return unlisten;
    });

    const registeredUnlisten = await listenToRuntimeBootstrapProgress(handler);

    expect(handler).toHaveBeenCalledOnce();
    expect(handler).toHaveBeenCalledWith(payload);
    expect(registeredUnlisten).toBe(unlisten);
  });

  it("keeps web evidence progress behind its typed event boundary", async () => {
    const payload = {
      operation: "fetch",
      evidence_id: "evidence-1",
      phase: "collecting",
      message: "Coletando fonte",
      at: "2026-08-21T12:00:00Z",
    };
    const handler = vi.fn();
    const unlisten = vi.fn();
    listenMock.mockImplementation(async (eventName, callback) => {
      expect(eventName).toBe("web-evidence-progress");
      callback({ payload } as never);
      return unlisten;
    });

    const registeredUnlisten = await listenToWebEvidenceProgress(handler);

    expect(handler).toHaveBeenCalledOnce();
    expect(handler).toHaveBeenCalledWith(payload);
    expect(registeredUnlisten).toBe(unlisten);
  });

  it("keeps editorial command requests behind one typed boundary", async () => {
    const runRequest = { run_id: "run-1" } as Parameters<typeof runEditorialSession>[0];
    const resumeRequest = { run_id: "run-1" } as Parameters<typeof resumeEditorialSession>[0];

    await runEditorialSession(runRequest);
    await resumeEditorialSession(resumeRequest);
    await stopEditorialSession("run-1");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "run_editorial_session", {
      request: runRequest,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "resume_editorial_session", {
      request: resumeRequest,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "stop_editorial_session", {
      runId: "run-1",
    });
  });

  it("preserves settings payload shapes", async () => {
    const config = { schema_version: 3 } as AiProviderConfig;
    const cloudflare = {
      account_id: "example-account",
      api_token: null,
      api_token_env_var: "EXAMPLE_TOKEN",
      persistence_database: "example-db",
      secret_store: "example-store",
    };
    const probe = {
      account_id: "example-account",
      api_token: null,
      api_token_env_var: "EXAMPLE_TOKEN",
      persistence_database: "example-db",
      publication_database: "example-publication-db",
      secret_store: "example-store",
    };

    await writeAiProviderConfig(config, cloudflare);
    await probeCloudflareCredentials(probe);
    await probeAiProviderCredentials(config);

    expect(invokeMock).toHaveBeenNthCalledWith(1, "write_ai_provider_config", {
      config,
      cloudflare,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "verify_cloudflare_credentials", {
      request: probe,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "verify_ai_provider_credentials", {
      config,
    });
  });
});
