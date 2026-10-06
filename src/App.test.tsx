import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { aiProviderRows } from "./constants";
import type {
  AiProviderConfig,
  BootstrapConfig,
  ResumableSessionInfo,
  RuntimeBootstrapPlan,
} from "./types";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const config: AiProviderConfig = {
  schema_version: 1,
  provider_mode: "api",
  credential_storage_mode: "local_json",
  agy_cli_project_id: null,
  openai_api_key: null,
  anthropic_api_key: null,
  gemini_api_key: null,
  deepseek_api_key: "fixture-credential",
  grok_api_key: null,
  perplexity_api_key: null,
  openai_api_key_remote: false,
  anthropic_api_key_remote: false,
  gemini_api_key_remote: false,
  deepseek_api_key_remote: false,
  grok_api_key_remote: false,
  perplexity_api_key_remote: false,
  openai_input_usd_per_million: null,
  openai_output_usd_per_million: null,
  anthropic_input_usd_per_million: null,
  anthropic_output_usd_per_million: null,
  gemini_input_usd_per_million: null,
  gemini_output_usd_per_million: null,
  deepseek_input_usd_per_million: null,
  deepseek_output_usd_per_million: null,
  grok_input_usd_per_million: null,
  grok_output_usd_per_million: null,
  perplexity_input_usd_per_million: null,
  perplexity_output_usd_per_million: null,
  cloudflare_secret_store_id: null,
  cloudflare_secret_store_name: null,
  updated_at: "2026-10-06T00:48:46Z",
};
const bootstrap = {
  credential_storage_mode: "local_json",
  cloudflare_api_token_source: "prompt_each_launch",
  cloudflare_api_token_env_var: "MAESTRO_CLOUDFLARE_API_TOKEN",
} as BootstrapConfig;

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function plan(detail: string, ready = true): RuntimeBootstrapPlan {
  return {
    schema_version: 1,
    plan_hash: detail,
    created_at: "2026-10-06T00:48:07Z",
    expires_at: "2026-10-06T01:00:00Z",
    dependencies: [
      {
        key: "deepseek_credential",
        label: "Credencial API DeepSeek",
        required: false,
        state: ready ? "ready" : "manual_action_required",
        detail,
        installed_version: null,
        latest_version: null,
        resolved_path: null,
        recommended_action_ids: ready ? [] : ["configure.deepseek.credential"],
      },
    ],
    actions: ready
      ? []
      : [
          {
            action_id: "configure.deepseek.credential",
            dependency_key: "deepseek_credential",
            kind: "manual",
            title: "Configurar credencial DeepSeek",
            description: "Abra os ajustes seguros.",
            source: "Maestro credential settings",
            command_preview: null,
            install_scope: "usuario",
            requires_elevation: false,
            requires_interaction: true,
          },
        ],
    required_ready: true,
    report_path: "data/bootstrap/current-plan.json",
    events_path: "data/bootstrap/events.ndjson",
  };
}

function navigate(name: "Setup" | "Ajustes" | "Sessao" | "Protocolos") {
  fireEvent.click(
    within(screen.getByRole("navigation", { name: "Principal" })).getByRole("button", { name }),
  );
}

beforeEach(() => {
  invoke.mockReset();
  invoke.mockImplementation(async (command: string, args?: { config?: AiProviderConfig }) => {
    if (command === "read_ai_provider_config") return { ...config };
    if (command === "read_bootstrap_config") return bootstrap;
    if (command === "read_cloudflare_env_snapshot") return { api_token_present: false };
    if (command === "runtime_bootstrap_plan") return plan("Fonte DeepSeek atual");
    if (command === "write_ai_provider_config")
      return { ...args?.config, updated_at: config.updated_at };
    if (command === "verify_ai_provider_credentials") {
      return {
        checked_at: config.updated_at,
        rows: aiProviderRows.map((provider) => ({
          label: provider.name,
          value: "API respondeu; credencial aceita",
          tone: "ok",
        })),
      };
    }
    if (command === "dependency_preflight")
      return { checks: [{ label: "Diagnostico antigo", value: "antigo", tone: "warn" }] };
    return null;
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("configuration and native Setup inventory custody", () => {
  it("shows native CLI readiness without inventing an AGY authentication dependency or action", async () => {
    const original = invoke.getMockImplementation();
    const agyDetail = "AGY 1.2.17; versao confirmada, autenticacao nao atestada pelo inventario";
    const cliPlan: RuntimeBootstrapPlan = {
      ...plan("CLI-ready native plan"),
      dependencies: ["webview2", "portable_data", "network", "claude", "codex", "agy"].map(
        (key) => ({
          key,
          label: key === "agy" ? "Antigravity CLI (agy)" : key,
          required: true,
          state: "ready",
          detail: key === "agy" ? agyDetail : `${key} pronto`,
          installed_version: key === "agy" ? "1.2.17" : null,
          latest_version: null,
          resolved_path: key === "agy" ? "C:\\official\\agy.exe" : null,
          recommended_action_ids: [],
        }),
      ),
      actions: [],
      required_ready: true,
    };
    invoke.mockImplementation((command: string, args?: { config?: AiProviderConfig }) => {
      if (command === "read_ai_provider_config")
        return Promise.resolve({ ...config, provider_mode: "cli" });
      if (command === "runtime_bootstrap_plan") return Promise.resolve(cliPlan);
      return original?.(command, args);
    });
    render(<App />);
    navigate("Setup");
    expect(await screen.findByText(agyDetail)).toBeInTheDocument();
    expect(
      screen.getByText("Todas as dependencias obrigatorias estao prontas."),
    ).toBeInTheDocument();
    expect(screen.getByText("Nenhuma acao necessaria neste momento.")).toBeInTheDocument();
    expect(
      screen.queryByText(/Autenticar.*Antigravity|Autenticacao.*Antigravity|agy_auth/i),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "Autorizar" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Iniciar handoff" })).toBeNull();
    expect(
      invoke.mock.calls.filter(([command]) => command === "execute_runtime_bootstrap_action"),
    ).toHaveLength(0);
  });

  it("invalidates obsolete actions during save and keeps late config loading from replacing the new inventory", async () => {
    const saved = deferred<AiProviderConfig>();
    const lateBootstrap = deferred<BootstrapConfig>();
    const original = invoke.getMockImplementation();
    let plans = 0;
    invoke.mockImplementation((command: string, args?: { config?: AiProviderConfig }) => {
      if (command === "read_bootstrap_config") return lateBootstrap.promise;
      if (command === "runtime_bootstrap_plan")
        return Promise.resolve(
          ++plans === 1 ? plan("Fonte antiga ausente", false) : plan("Fonte DeepSeek salva"),
        );
      if (command === "write_ai_provider_config") return saved.promise;
      return original?.(command, args);
    });
    render(<App />);
    navigate("Setup");
    expect(await screen.findByText("Fonte antiga ausente")).toBeInTheDocument();
    navigate("Ajustes");
    await waitFor(() =>
      expect(screen.getByLabelText("DeepSeek API key")).toHaveValue("fixture-credential"),
    );
    fireEvent.click(screen.getByRole("button", { name: "Salvar provedores" }));
    navigate("Setup");
    expect(screen.queryByText("Configurar credencial DeepSeek")).toBeNull();
    expect(screen.queryByText("Fonte antiga ausente")).toBeNull();
    await act(async () => saved.resolve(config));
    expect(await screen.findByText("Fonte DeepSeek salva")).toBeInTheDocument();
    await act(async () => lateBootstrap.resolve(bootstrap));
    expect(screen.getByText("Fonte DeepSeek salva")).toBeInTheDocument();
    expect(
      invoke.mock.calls.filter(([command]) => command === "dependency_preflight"),
    ).toHaveLength(0);
  });

  it("rejects a pre-save plan response without clearing a newer inventory request's busy state", async () => {
    const old = deferred<RuntimeBootstrapPlan>();
    const fresh = deferred<RuntimeBootstrapPlan>();
    const original = invoke.getMockImplementation();
    let plans = 0;
    invoke.mockImplementation((command: string, args?: { config?: AiProviderConfig }) => {
      if (command === "runtime_bootstrap_plan") return ++plans === 1 ? old.promise : fresh.promise;
      return original?.(command, args);
    });
    render(<App />);
    navigate("Ajustes");
    await waitFor(() =>
      expect(screen.getByLabelText("DeepSeek API key")).toHaveValue("fixture-credential"),
    );
    fireEvent.click(screen.getByRole("button", { name: "CLI" }));
    await waitFor(() => expect(plans).toBe(2));
    navigate("Setup");
    await act(async () => old.resolve(plan("Resposta obsoleta", false)));
    expect(screen.queryByText("Resposta obsoleta")).toBeNull();
    expect(screen.getByTitle("Reverificar e gerar novo plano")).toBeDisabled();
    await act(async () => fresh.resolve(plan("Inventario do modo salvo")));
    expect(screen.getByText("Inventario do modo salvo")).toBeInTheDocument();
    expect(screen.getByTitle("Reverificar e gerar novo plano")).toBeEnabled();
  });

  it("invalidates only an actually changed credential's prior API acceptance", async () => {
    render(<App />);
    navigate("Ajustes");
    await waitFor(() =>
      expect(screen.getByLabelText("DeepSeek API key")).toHaveValue("fixture-credential"),
    );
    fireEvent.click(screen.getByRole("button", { name: "Verificar APIs" }));
    await waitFor(() =>
      expect(screen.getAllByText("API respondeu; credencial aceita")).toHaveLength(6),
    );
    fireEvent.change(screen.getByLabelText("DeepSeek API key"), {
      target: { value: " fixture-credential " },
    });
    expect(screen.getAllByText("API respondeu; credencial aceita")).toHaveLength(6);
    fireEvent.change(screen.getByLabelText("DeepSeek API key"), {
      target: { value: "fixture-other-credential" },
    });
    expect(screen.getByText("credencial alterada; verificacao pendente")).toBeInTheDocument();
    expect(screen.getAllByText("API respondeu; credencial aceita")).toHaveLength(5);
  });

  it("loads and saves the explicit AGY project through the canonical native configuration field", async () => {
    const original = invoke.getMockImplementation();
    const selectedProject = "af394913-0605-4e65-8c18-b749e4743572";
    const updatedProject = "4d16616e-b677-4ed5-96c5-26ff29d56851";
    invoke.mockImplementation((command: string, args?: { config?: AiProviderConfig }) => {
      if (command === "read_ai_provider_config")
        return Promise.resolve({
          ...config,
          provider_mode: "cli",
          agy_cli_project_id: selectedProject,
        });
      return original?.(command, args);
    });
    render(<App />);
    navigate("Ajustes");
    const projectInput = screen.getByLabelText("ID do projeto nativo do AGY CLI");
    await waitFor(() => expect(projectInput).toHaveValue(selectedProject));
    fireEvent.change(projectInput, { target: { value: ` ${updatedProject} ` } });
    fireEvent.click(screen.getByRole("button", { name: "Salvar provedores" }));
    await waitFor(() => expect(projectInput).toHaveValue(updatedProject));
    expect(invoke).toHaveBeenCalledWith(
      "write_ai_provider_config",
      expect.objectContaining({
        config: expect.objectContaining({ agy_cli_project_id: updatedProject }),
      }),
    );
  });

  it("preserves an active action's custody and derives its refreshed inventory from the returned plan", async () => {
    const action = deferred<unknown>();
    const original = invoke.getMockImplementation();
    invoke.mockImplementation((command: string, args?: { config?: AiProviderConfig }) => {
      if (command === "runtime_bootstrap_plan")
        return Promise.resolve(plan("Ajuste pendente", false));
      if (command === "execute_runtime_bootstrap_action") return action.promise;
      return original?.(command, args);
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<App />);
    navigate("Setup");
    fireEvent.click(await screen.findByRole("button", { name: "Iniciar handoff" }));
    navigate("Ajustes");
    expect(screen.getByRole("button", { name: "Salvar provedores" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Salvando" })).toBeNull();
    expect(screen.getByRole("button", { name: "API" })).toBeDisabled();
    expect(
      invoke.mock.calls.filter(([command]) => command === "write_ai_provider_config"),
    ).toHaveLength(0);
    await act(async () =>
      action.resolve({
        status: "completed",
        message: "Configuracao concluida",
        refreshed_plan: plan("Fonte atualizada pela acao"),
      }),
    );
    navigate("Setup");
    expect(screen.getByText("Fonte atualizada pela acao")).toBeInTheDocument();
    expect(screen.queryByText("Ajuste pendente")).toBeNull();
  });
});

describe("protocol and attachment input custody", () => {
  const protocolText = "SYNTHETIC_PROTOCOL_RULE ".repeat(12);
  // These tests exercise File ownership; the real browser proof uses native SHA-256.
  const fixtureCrypto = { subtle: { digest: async () => new Uint8Array(32).buffer } };

  function protocolFile(name: string, text: Promise<string>) {
    const file = new File([protocolText], name, { type: "text/markdown" });
    Object.defineProperty(file, "text", { value: () => text });
    return file;
  }

  function attachmentFile(name: string, bytes: Promise<ArrayBuffer>) {
    const file = new File(["SYNTHETIC_EVIDENCE"], name, { type: "text/plain" });
    Object.defineProperty(file, "arrayBuffer", { value: () => bytes });
    return file;
  }

  async function cliComposer() {
    vi.stubGlobal("crypto", fixtureCrypto);
    const original = invoke.getMockImplementation();
    invoke.mockImplementation((command: string, args?: { config?: AiProviderConfig }) => {
      if (command === "read_ai_provider_config")
        return Promise.resolve({ ...config, provider_mode: "cli" });
      if (command === "run_editorial_session") return new Promise(() => {});
      return original?.(command, args);
    });
    const view = render(<App />);
    const controls = screen.getByLabelText("Controles da sessao");
    await waitFor(() =>
      expect(within(controls).getByRole("button", { name: "DeepSeek" })).toBeDisabled(),
    );
    for (const name of ["Codex", "Gemini"]) {
      fireEvent.click(within(controls).getByRole("button", { name }));
    }
    navigate("Protocolos");
    fireEvent.change(view.container.querySelector('input[type="file"]') as HTMLInputElement, {
      target: { files: [protocolFile("initial-protocol.md", Promise.resolve(protocolText))] },
    });
    expect(await screen.findByText("initial-protocol.md")).toBeInTheDocument();
    navigate("Sessao");
    return view;
  }

  it("keeps the latest selected protocol when an older file finishes afterward", async () => {
    vi.stubGlobal("crypto", fixtureCrypto);
    const old = deferred<string>();
    const view = render(<App />);
    navigate("Protocolos");
    const input = view.container.querySelector('input[type="file"]') as HTMLInputElement;
    fireEvent.change(input, { target: { files: [protocolFile("old.md", old.promise)] } });
    expect(screen.getByRole("button", { name: "Preparando" })).toBeDisabled();
    fireEvent.change(input, {
      target: { files: [protocolFile("new.md", Promise.resolve(protocolText))] },
    });
    expect(await screen.findByText("new.md")).toBeInTheDocument();
    await act(async () => old.resolve("OLD_RULE ".repeat(30)));
    expect(screen.getByText("new.md")).toBeInTheDocument();
    expect(screen.queryByText("old.md")).toBeNull();
  });

  it("keeps submission blocked when a superseded protocol finishes before the current selection", async () => {
    vi.stubGlobal("crypto", fixtureCrypto);
    const old = deferred<string>();
    const current = deferred<string>();
    const view = render(<App />);
    navigate("Protocolos");
    const input = view.container.querySelector('input[type="file"]') as HTMLInputElement;
    fireEvent.change(input, { target: { files: [protocolFile("old.md", old.promise)] } });
    fireEvent.change(input, { target: { files: [protocolFile("current.md", current.promise)] } });
    await act(async () => old.resolve(protocolText));
    expect(screen.getByRole("button", { name: "Preparando" })).toBeDisabled();
    expect(screen.queryByText("old.md")).toBeNull();
    await act(async () => current.resolve(protocolText));
    expect(await screen.findByText("current.md")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Iniciar sessao" })).toBeEnabled();
  });

  it("waits for attachment bytes, preserves existing files and submits the complete owned list", async () => {
    const view = await cliComposer();
    const input = view.container.querySelector('input[type="file"]') as HTMLInputElement;
    const existing = attachmentFile("existing.txt", Promise.resolve(new Uint8Array([65]).buffer));
    fireEvent.change(input, { target: { files: [existing] } });
    expect(await screen.findByText(/existing\.txt/)).toBeInTheDocument();
    const pending = deferred<ArrayBuffer>();
    fireEvent.change(input, {
      target: { files: [attachmentFile("pending.txt", pending.promise)] },
    });
    expect(input).toBeDisabled();
    expect(screen.getByRole("button", { name: /existing\.txt/ })).toBeDisabled();
    expect(screen.getAllByRole("button", { name: "Preparando" })).toHaveLength(2);
    for (const button of screen.getAllByRole("button", { name: "Retomar" })) {
      expect(button).toBeDisabled();
    }
    fireEvent.click(screen.getAllByRole("button", { name: "Preparando" })[0] as HTMLElement);
    fireEvent.click(screen.getByRole("button", { name: /existing\.txt/ }));
    const ignoredRead = vi.fn(async () => new Uint8Array([67]).buffer);
    const ignored = new File(["ignored"], "ignored.txt", { type: "text/plain" });
    Object.defineProperty(ignored, "arrayBuffer", { value: ignoredRead });
    fireEvent.change(input, { target: { files: [ignored] } });
    expect(ignoredRead).not.toHaveBeenCalled();
    expect(invoke.mock.calls.some(([command]) => command === "run_editorial_session")).toBe(false);
    await act(async () => pending.resolve(new Uint8Array([66]).buffer));
    expect(await screen.findByText(/pending\.txt/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /existing\.txt/ })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Submeter" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "run_editorial_session",
        expect.objectContaining({
          request: expect.objectContaining({
            protocol_name: "initial-protocol.md",
            attachments: [
              expect.objectContaining({ name: "existing.txt", data_base64: "QQ==" }),
              expect.objectContaining({ name: "pending.txt", data_base64: "Qg==" }),
            ],
          }),
        }),
      ),
    );
  });

  it("releases a failed attachment read without removing previously accepted evidence", async () => {
    const view = await cliComposer();
    const input = view.container.querySelector('input[type="file"]') as HTMLInputElement;
    fireEvent.change(input, {
      target: {
        files: [attachmentFile("existing.txt", Promise.resolve(new Uint8Array([65]).buffer))],
      },
    });
    expect(await screen.findByText(/existing\.txt/)).toBeInTheDocument();
    const failing = deferred<ArrayBuffer>();
    fireEvent.change(input, {
      target: { files: [attachmentFile("unreadable.txt", failing.promise)] },
    });
    await act(async () => failing.reject(new Error("Synthetic local file read failure")));
    expect(await screen.findByText("Falha ao ler anexos")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Submeter" })).toBeEnabled();
    expect(screen.getByRole("button", { name: /existing\.txt/ })).toBeEnabled();
    expect(screen.queryByText(/unreadable\.txt/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Submeter" }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "run_editorial_session",
        expect.objectContaining({
          request: expect.objectContaining({
            attachments: [expect.objectContaining({ name: "existing.txt", data_base64: "QQ==" })],
          }),
        }),
      ),
    );
  });

  it("does not resume with a protocol captured before a delayed session lookup", async () => {
    const view = await cliComposer();
    const lookup = deferred<ResumableSessionInfo[]>();
    const original = invoke.getMockImplementation();
    invoke.mockImplementation((command: string, args?: { config?: AiProviderConfig }) => {
      if (command === "list_resumable_sessions") return lookup.promise;
      return original?.(command, args);
    });
    fireEvent.click(screen.getAllByRole("button", { name: "Retomar" })[0] as HTMLElement);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("list_resumable_sessions"));
    navigate("Protocolos");
    fireEvent.change(view.container.querySelector('input[type="file"]') as HTMLInputElement, {
      target: { files: [protocolFile("replacement.md", Promise.resolve(protocolText))] },
    });
    expect(await screen.findByText("replacement.md")).toBeInTheDocument();
    await act(async () =>
      lookup.resolve([
        {
          run_id: "synthetic-interrupted-run",
          session_name: "Synthetic interrupted session",
          session_dir: "data/sessions/synthetic-interrupted-run",
          prompt_path: "data/sessions/synthetic-interrupted-run/prompt.md",
          protocol_path: "data/sessions/synthetic-interrupted-run/protocol.md",
          draft_path: null,
          final_markdown_path: null,
          next_round: 2,
          last_activity_unix: 1,
          artifact_count: 1,
          protocol_lines: 12,
          status: "interrupted",
          saved_active_agents: ["claude"],
          saved_initial_agent: "claude",
          saved_max_session_cost_usd: null,
          saved_max_session_minutes: null,
        },
      ]),
    );
    expect(invoke.mock.calls.some(([command]) => command === "resume_editorial_session")).toBe(
      false,
    );
    navigate("Sessao");
    expect(screen.getByText("Entradas alteradas")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Retomar" })[0]).toBeEnabled();
    navigate("Protocolos");
    expect(screen.getByText("replacement.md")).toBeInTheDocument();
  });

  it("discards a pending protocol import after unmount without logging it as accepted", async () => {
    vi.stubGlobal("crypto", fixtureCrypto);
    const pending = deferred<string>();
    const view = render(<App />);
    navigate("Protocolos");
    fireEvent.change(view.container.querySelector('input[type="file"]') as HTMLInputElement, {
      target: { files: [protocolFile("abandoned.md", pending.promise)] },
    });
    view.unmount();
    await act(async () => pending.resolve(protocolText));
    expect(
      invoke.mock.calls.some(
        ([command, args]) =>
          command === "write_log_event" && args.event.category === "protocol.imported",
      ),
    ).toBe(false);
  });
});
