import type { ChangeEvent } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import packageJson from "../package.json";
import { AppSidebar } from "./app/AppSidebar";
import { AppTopbar } from "./app/AppTopbar";
import {
  aiProviderRows,
  attachmentLimits,
  defaultActiveAgents,
  finalArtifacts,
  idleActivityFeed,
  idleOperation,
  idlePhases,
  initialAgentOptions,
  initialAgents,
  initialAiProviderChecks,
  initialBootstrapChecks,
  initialCloudflarePermissionChecks,
  initialDiscussionRounds,
  initialEvidenceRows,
  initialProtocolReadingGates,
  navItems,
} from "./constants";
import { logEvent } from "./diagnostics";
import { AgentsScreen } from "./features/agents/AgentsScreen";
import { EvidenceScreen } from "./features/evidence/EvidenceScreen";
import { ProtocolsScreen } from "./features/protocols/ProtocolsScreen";
import { ResumeDialog } from "./features/session/ResumeDialog";
import { SessionScreen } from "./features/session/SessionScreen";
import { AiProviderSettingsPanel } from "./features/settings/AiProviderSettingsPanel";
import { CloudflareSettingsPanel } from "./features/settings/CloudflareSettingsPanel";
import { SettingsScreen } from "./features/settings/SettingsScreen";
import { SetupScreen } from "./features/setup/SetupScreen";
import {
  attachmentDeliveryPlan,
  formatBrazilDateTime,
  formatElapsedTime,
  humanizeAgentStatus,
  humanizeRunStatus,
  latestAgentCards,
  latestProtocolGateItems,
  operationMeterLabel,
  sha256,
  summarizeAgentResults,
} from "./helpers";
import { useEscapeKey } from "./hooks/useEscapeKey";
import {
  listResumableSessions,
  resumeEditorialSession,
  runEditorialSession,
  stopEditorialSession,
} from "./services/editorial";
import { auditAbntCitations, auditLinks } from "./services/evidence";
import {
  listenToNativeLogs,
  listenToRuntimeBootstrapProgress,
  type NativeLogTone,
} from "./services/nativeEvents";
import {
  controlRuntimeBootstrapAction,
  createRuntimeBootstrapPlan,
  dependencyPreflight,
  executeRuntimeBootstrapAction,
  openDataFile,
  readBootstrapConfig,
  readCloudflareEnvSnapshot,
  writeBootstrapConfig,
} from "./services/runtime";
import {
  probeAiProviderCredentials,
  probeCloudflareCredentials,
  readAiProviderConfig,
  writeAiProviderConfig,
} from "./services/settings";
import type {
  ActiveSection,
  ActivityItem,
  AgentCard,
  AgentState,
  AiCredentialKey,
  AiProviderConfig,
  AiProviderProbeRow,
  BootstrapCheckRow,
  BootstrapConfig,
  CitationAuditResult,
  CitationManifest,
  CloudflareEnvSnapshot,
  CloudflarePermissionRow,
  CloudflareProviderStorageRequest,
  CloudflareTokenSource,
  CredentialStorageMode,
  DiscussionRound,
  EvidenceRow,
  InitialAgentKey,
  LinkAuditResult,
  OperationSnapshot,
  PhaseItem,
  PromptAttachmentPayload,
  ProtocolReadingGate,
  ProtocolSnapshot,
  ProviderMode,
  ProviderRateKey,
  ResumableSessionInfo,
  RuntimeBootstrapActionResult,
  RuntimeBootstrapDisposition,
  RuntimeBootstrapPlan,
  RuntimeBootstrapProgressEvent,
  SessionRunOptions,
  SettingsTab,
  VerbosityMode,
} from "./types";

const APP_VERSION = `v${packageJson.version}`;

type ActiveAgentNow = {
  name: string;
  role: string;
  detail: string;
  state: "idle" | "running" | "finished";
};

const agentIsApiOnly = (agent: InitialAgentKey) =>
  agent === "deepseek" || agent === "grok" || agent === "perplexity";

function citationManifestsFromAttachments(attachments: PromptAttachmentPayload[]) {
  let current: CitationManifest | null = null;
  let previous: CitationManifest | null = null;
  for (const attachment of attachments) {
    const name = attachment.name.toLocaleLowerCase("pt-BR");
    const mediaType = attachment.media_type?.toLocaleLowerCase("pt-BR") ?? "";
    const explicitlyNamed =
      name.includes("citation-manifest") ||
      name.includes("citation_manifest") ||
      name.includes("manifesto-citacoes") ||
      name.includes("manifesto_citacoes");
    if (!name.endsWith(".json") && mediaType !== "application/json" && !explicitlyNamed) {
      continue;
    }
    let parsed: unknown;
    try {
      const binary = atob(attachment.data_base64);
      const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
      parsed = JSON.parse(new TextDecoder().decode(bytes));
    } catch (error) {
      if (explicitlyNamed) {
        throw new Error(`Manifesto de citacoes invalido: ${String(error)}`);
      }
      continue;
    }
    if (
      !parsed ||
      typeof parsed !== "object" ||
      !("schema_version" in parsed) ||
      parsed.schema_version !== "citation_manifest.v1"
    ) {
      if (explicitlyNamed) {
        throw new Error("O manifesto de citacoes deve usar citation_manifest.v1.");
      }
      continue;
    }
    const isPrevious = name.includes("previous") || name.includes("anterior");
    if (isPrevious) {
      if (previous) throw new Error("Mais de um manifesto de citacoes anterior foi anexado.");
      previous = parsed as CitationManifest;
    } else {
      if (current) throw new Error("Mais de um manifesto de citacoes atual foi anexado.");
      current = parsed as CitationManifest;
    }
  }
  return { current, previous };
}

export function App() {
  const inputRef = useRef<HTMLInputElement>(null);
  const [protocol, setProtocol] = useState<ProtocolSnapshot>({
    name: "Nenhum protocolo carregado",
    size: 0,
    lines: 0,
    hash: "aguardando importacao",
  });
  const [protocolText, setProtocolText] = useState("");
  const [sessionName, setSessionName] = useState("Artigo academico sem titulo");
  const [verbosity, setVerbosity] = useState<VerbosityMode>("detalhado");
  const [editorialPrompt, setEditorialPrompt] = useState(
    "Escreva um artigo acadêmico sobre [...], seguindo rigorosa e integralmente o protocolo editorial ativo.",
  );
  const [showPostEditor, setShowPostEditor] = useState(false);
  const [mainSiteHtml, setMainSiteHtml] = useState(
    '<h1>Artigo em preparacao</h1><p style="text-align: justify">Texto inicial para edicao com o mesmo PostEditor usado pelo MainSite.</p>',
  );
  const [providerMode, setProviderMode] = useState<ProviderMode>("hybrid");
  const [initialAgent, setInitialAgent] = useState<InitialAgentKey>("claude");
  const [activeAgents, setActiveAgents] = useState<InitialAgentKey[]>(defaultActiveAgents);
  const [maxSessionCostUsd, setMaxSessionCostUsd] = useState("");
  const [maxSessionMinutes, setMaxSessionMinutes] = useState("");
  const [promptAttachments, setPromptAttachments] = useState<PromptAttachmentPayload[]>([]);
  const [sessionLinks, setSessionLinks] = useState("");
  const [credentialStorageMode, setCredentialStorageMode] =
    useState<CredentialStorageMode>("local_json");
  const [activeSection, setActiveSection] = useState<ActiveSection>("session");
  const [activeSettingsTab, setActiveSettingsTab] = useState<SettingsTab>("providers");
  const [cloudflareAccountId, setCloudflareAccountId] = useState("");
  const [cloudflareApiToken, setCloudflareApiToken] = useState("");
  const [cloudflareTokenSource, setCloudflareTokenSource] =
    useState<CloudflareTokenSource>("prompt_each_launch");
  const [cloudflareTokenEnvVar, setCloudflareTokenEnvVar] = useState(
    "MAESTRO_CLOUDFLARE_API_TOKEN",
  );
  const [cloudflareEnvSnapshot, setCloudflareEnvSnapshot] = useState<CloudflareEnvSnapshot | null>(
    null,
  );
  const [aiCredentials, setAiCredentials] = useState<Record<AiCredentialKey, string>>({
    openai: "",
    anthropic: "",
    gemini: "",
    deepseek: "",
    grok: "",
    perplexity: "",
  });
  const [providerInputUsdPerMillion, setProviderInputUsdPerMillion] = useState<
    Record<ProviderRateKey, string>
  >({
    openai: "",
    anthropic: "",
    gemini: "",
    deepseek: "",
    grok: "",
    perplexity: "",
  });
  const [providerOutputUsdPerMillion, setProviderOutputUsdPerMillion] = useState<
    Record<ProviderRateKey, string>
  >({
    openai: "",
    anthropic: "",
    gemini: "",
    deepseek: "",
    grok: "",
    perplexity: "",
  });
  const [sessionRunId, setSessionRunId] = useState<string | null>(null);
  const [lastSessionMinutesPath, setLastSessionMinutesPath] = useState<string | null>(null);
  const [operation, setOperation] = useState<OperationSnapshot>(idleOperation);
  // True after the operator confirms the "Parar sessao" button until the
  // backend's session loop observes the cancellation token and returns
  // STOPPED_BY_USER. Disables the button to prevent duplicate signals.
  const [isStopRequested, setIsStopRequested] = useState(false);
  const [phaseItems, setPhaseItems] = useState<PhaseItem[]>(idlePhases);
  const [activityItems, setActivityItems] = useState<ActivityItem[]>(idleActivityFeed);
  const [discussionItems, setDiscussionItems] =
    useState<DiscussionRound[]>(initialDiscussionRounds);
  const [agentCards, setAgentCards] = useState<AgentCard[]>(initialAgents);
  const [activeAgentNow, setActiveAgentNow] = useState<ActiveAgentNow | null>(null);
  const [evidenceRows, setEvidenceRows] = useState<EvidenceRow[]>(initialEvidenceRows);
  const [linkAuditRows, setLinkAuditRows] = useState<LinkAuditResult["rows"]>([]);
  const [citationAuditResult, setCitationAuditResult] = useState<CitationAuditResult | null>(null);
  const [protocolGateItems, setProtocolGateItems] = useState<ProtocolReadingGate[]>(
    initialProtocolReadingGates,
  );
  const [cloudflarePermissionRows, setCloudflarePermissionRows] = useState<
    CloudflarePermissionRow[]
  >(initialCloudflarePermissionChecks);
  const [aiProviderRowsState, setAiProviderRowsState] =
    useState<AiProviderProbeRow[]>(initialAiProviderChecks);
  const [bootstrapRows, setBootstrapRows] = useState<BootstrapCheckRow[]>(initialBootstrapChecks);
  const [runtimeBootstrapPlan, setRuntimeBootstrapPlan] = useState<RuntimeBootstrapPlan | null>(
    null,
  );
  const [runtimeBootstrapResult, setRuntimeBootstrapResult] =
    useState<RuntimeBootstrapActionResult | null>(null);
  const [runtimeBootstrapProgress, setRuntimeBootstrapProgress] = useState<
    RuntimeBootstrapProgressEvent[]
  >([]);
  const [isPlanningRuntimeBootstrap, setIsPlanningRuntimeBootstrap] = useState(false);
  const [activeRuntimeBootstrapActionId, setActiveRuntimeBootstrapActionId] = useState<
    string | null
  >(null);
  const [bootstrapConfigStatus, setBootstrapConfigStatus] = useState(
    "bootstrap.json ainda nao carregado",
  );
  const [aiConfigStatus, setAiConfigStatus] = useState("Chaves ainda nao carregadas");
  const [isVerifyingCloudflare, setIsVerifyingCloudflare] = useState(false);
  const [isSavingAiConfig, setIsSavingAiConfig] = useState(false);
  const [isVerifyingAiProviders, setIsVerifyingAiProviders] = useState(false);
  const [isAuditingEvidence, setIsAuditingEvidence] = useState(false);
  const [resumeCandidates, setResumeCandidates] = useState<ResumableSessionInfo[]>([]);
  const [showResumePicker, setShowResumePicker] = useState(false);
  const [isResumeLoading, setIsResumeLoading] = useState(false);
  const [useLoadedProtocolForResume, setUseLoadedProtocolForResume] = useState(false);
  const sessionRunIdRef = useRef<string | null>(null);

  // v0.3.14 / audit closure (MEDIUM): ESC dismissal on the ResumeDialog at
  // line 2574. Mirrors the existing Close button (line 2582) — no new
  // dismissal path, no new state. Hook gated by `showResumePicker` so the
  // window listener is detached when the dialog is hidden. In-place edit
  // per docs/code-split-plan.md ("future splits should start with pure
  // helpers, ... without mixing large refactors with behavior changes").
  const handleResumeDialogEscape = useCallback(() => {
    setShowResumePicker(false);
  }, []);
  useEscapeKey(handleResumeDialogEscape, showResumePicker);

  useEffect(() => {
    sessionRunIdRef.current = sessionRunId;
  }, [sessionRunId]);

  useEffect(() => {
    let unsubscribe: (() => void) | null = null;
    let disposed = false;

    const roleLabel = (role: string | undefined) => {
      if (role === "draft") return "redacao";
      if (role === "review") return "revisao";
      if (role === "revision") return "reescrita";
      return role || "turno editorial";
    };

    const toneToState = (tone: NativeLogTone): AgentState => {
      if (tone === "ok") return "ready";
      if (tone === "warn") return "evidence";
      if (tone === "blocked" || tone === "error") return "blocked";
      return "evidence";
    };

    void listenToNativeLogs((payload) => {
      const { category, context } = payload;
      if (!context?.run_id || context.run_id !== sessionRunIdRef.current) return;

      if (category === "session.agent.started") {
        const name = context.agent ?? "Agente";
        const role = roleLabel(context.role);
        const detail = `${role} em andamento${context.cli ? ` via ${context.cli}` : ""}`;
        setActiveAgentNow({ name, role, detail, state: "running" });
        setAgentCards((current) =>
          current.map((agent) =>
            agent.name === name
              ? { ...agent, state: "running", note: detail }
              : agent.name === "Maestro"
                ? agent
                : agent.state === "running"
                  ? { ...agent, state: "evidence", note: "aguardando seu turno no circuito" }
                  : agent,
          ),
        );
        return;
      }

      if (category === "session.agent.running") {
        const name = context.agent ?? "Agente";
        const role = roleLabel(context.role);
        const elapsed =
          context.elapsed_seconds == null
            ? ""
            : ` ha ${formatElapsedTime(context.elapsed_seconds)}`;
        const detail = `${role} em andamento${elapsed}${context.cli ? ` via ${context.cli}` : ""}`;
        setActiveAgentNow({ name, role, detail, state: "running" });
        return;
      }

      if (category === "session.agent.finished") {
        const name = context.agent ?? "Agente";
        const role = roleLabel(context.role);
        const status = context.status ? humanizeAgentStatus(context.status) : "turno finalizado";
        setActiveAgentNow({ name, role, detail: status, state: "finished" });
        setAgentCards((current) =>
          current.map((agent) =>
            agent.name === name
              ? { ...agent, state: toneToState(context.tone), note: `${role}: ${status}` }
              : agent,
          ),
        );
        return;
      }

      if (category === "session.editorial.completed" || category === "session.editorial.failed") {
        setActiveAgentNow(null);
      }
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        unsubscribe = unlisten;
      }
    });

    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, []);

  useEffect(() => {
    let unsubscribe: (() => void) | null = null;
    let disposed = false;

    void listenToRuntimeBootstrapProgress((payload) => {
      setRuntimeBootstrapProgress((current) => [payload, ...current].slice(0, 40));
    }).then((unlisten) => {
      if (disposed) {
        unlisten();
      } else {
        unsubscribe = unlisten;
      }
    });

    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, []);

  const readyCount = useMemo(
    () => agentCards.filter((agent) => agent.state === "ready").length,
    [agentCards],
  );
  const visibleActivity = useMemo(() => {
    if (verbosity === "resumo") return activityItems.slice(0, 1);
    if (verbosity === "detalhado")
      return activityItems.filter((item) => item.level !== "diagnostic");
    return activityItems;
  }, [activityItems, verbosity]);
  const isRunPreparing = operation.status === "preparing" || operation.status === "running";
  const runActionLabel =
    operation.status === "paused" ||
    operation.status === "blocked" ||
    operation.status === "completed"
      ? "Nova sessao"
      : "Iniciar sessao";
  const formalState = humanizeRunStatus(operation.status);
  const linkEvidenceState =
    evidenceRows.find((item) => item.label === "Links")?.value ?? "nao iniciado";
  const activeNavItem = navItems.find((item) => item.section === activeSection) ?? navItems[0];
  const cloudflareTokenAvailable =
    cloudflareApiToken.length > 0 || Boolean(cloudflareEnvSnapshot?.api_token_present);
  const operationIndeterminate = operation.status === "running";
  const operationProgressLabel = operationMeterLabel(operation.status);
  const hasLoadedProtocolForResume =
    protocolText.trim().length >= 100 && protocol.hash !== "aguardando importacao";
  const initialAgentLabel =
    initialAgentOptions.find((option) => option.key === initialAgent)?.label ?? "Claude";
  const activeAgentLabels = activeAgents
    .map((agent) => initialAgentOptions.find((option) => option.key === agent)?.label ?? agent)
    .join(", ");
  const attachmentTotalBytes = promptAttachments.reduce(
    (total, item) => total + item.size_bytes,
    0,
  );
  const providerForAgent: Record<InitialAgentKey, AiCredentialKey> = {
    claude: "anthropic",
    codex: "openai",
    gemini: "gemini",
    deepseek: "deepseek",
    grok: "grok",
    perplexity: "perplexity",
  };
  const agentUsesApi = (agent: InitialAgentKey) => {
    if (providerMode === "api") return true;
    if (providerMode === "cli") return false;
    // "hybrid" is deterministic by agent identity: DeepSeek, Grok and
    // Perplexity go API (no CLI integration in maestro-app); other peers stay
    // on CLI.
    return agentIsApiOnly(agent);
  };
  const providerRatesConfigured = (provider: AiCredentialKey) =>
    providerInputUsdPerMillion[provider].trim().length > 0 &&
    providerOutputUsdPerMillion[provider].trim().length > 0;
  const agentsMissingCostRates = activeAgents.filter(
    (agent) => agentUsesApi(agent) && !providerRatesConfigured(providerForAgent[agent]),
  );
  const costRatesRequired = agentsMissingCostRates.length > 0;
  const apiAgentsSelected = activeAgents.filter((agent) => agentUsesApi(agent));
  const apiCostLimitRequired =
    apiAgentsSelected.length > 0 && maxSessionCostUsd.trim().length === 0;
  const activeApiAttachmentProviders = activeAgents
    .filter((agent) => agentUsesApi(agent))
    .map((agent) => providerForAgent[agent])
    .filter((provider, index, providers) => providers.indexOf(provider) === index);
  const attachmentDeliveryPlans = promptAttachments.map((attachment) =>
    attachmentDeliveryPlan(attachment, activeApiAttachmentProviders),
  );

  useEffect(() => {
    if (!activeAgents.includes(initialAgent)) {
      setInitialAgent(activeAgents[0] ?? "claude");
    }
  }, [activeAgents, initialAgent]);

  useEffect(() => {
    // CLI mode is incompatible with API-only peers (DeepSeek, Grok and Perplexity).
    // Defense in depth: catches config-load AND resume-contract paths that call
    // setActiveAgents/setInitialAgent directly while providerMode is already 'cli'
    // (peer review v0.3.38: codex + deepseek raised this — providerMode-only deps
    // would miss saved-contract restore that injects API-only peers without flipping mode).
    // Reads activeAgents/initialAgent directly (not via setState updater closure)
    // so the React-hooks/preserve-manual-memoization lint sees them as real deps;
    // both setState calls are guarded so no render loop is possible.
    if (providerMode !== "cli") return;
    if (activeAgents.some(agentIsApiOnly)) {
      const filtered = activeAgents.filter((agent) => !agentIsApiOnly(agent));
      setActiveAgents(filtered.length === 0 ? ["claude"] : filtered);
    }
    if (agentIsApiOnly(initialAgent)) {
      setInitialAgent("claude");
    }
  }, [providerMode, activeAgents, initialAgent]);

  useEffect(() => {
    void logEvent({
      level: "info",
      category: "ui.session.loaded",
      message: "editorial dashboard loaded",
      context: {
        session_name: sessionName,
        protocol_name: protocol.name,
        formal_state: "auditoria_bibliografica",
      },
    });
  }, []);

  useEffect(() => {
    void loadBootstrapConfig();
    void loadAiProviderConfig();
    void refreshRuntimeBootstrapPlan();
  }, []);

  function activityTimestamp() {
    return new Date().toLocaleTimeString("pt-BR", {
      timeZone: "America/Sao_Paulo",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }

  function appendActivity(item: Omit<ActivityItem, "time">) {
    setActivityItems((current) => [{ ...item, time: activityTimestamp() }, ...current].slice(0, 8));
  }

  async function refreshRuntimeBootstrapPlan() {
    setIsPlanningRuntimeBootstrap(true);
    try {
      const plan = await createRuntimeBootstrapPlan();
      setRuntimeBootstrapPlan(plan);
      setBootstrapRows(
        plan.dependencies.map((dependency) => ({
          label: dependency.label,
          value: dependency.detail,
          tone:
            dependency.state === "ready"
              ? "ok"
              : dependency.required && dependency.state === "missing"
                ? "blocked"
                : dependency.state === "outdated" || dependency.state === "misconfigured"
                  ? "warn"
                  : "pending",
        })),
      );
      appendActivity({
        level: "detail",
        title: "Plano de setup atualizado",
        detail: `${plan.dependencies.length.toLocaleString("pt-BR")} dependencias; ${plan.actions.length.toLocaleString("pt-BR")} acoes propostas.`,
      });
      void logEvent({
        level: "info",
        category: "runtime.bootstrap.plan_loaded",
        message: "runtime bootstrap plan loaded",
        context: {
          plan_hash: plan.plan_hash,
          dependencies: plan.dependencies.length,
          actions: plan.actions.length,
          required_ready: plan.required_ready,
        },
      });
    } catch (error) {
      appendActivity({
        level: "diagnostic",
        title: "Falha ao planejar setup",
        detail: "Nenhuma alteracao foi executada. Consulte o log desta execucao.",
      });
      void logEvent({
        level: "error",
        category: "runtime.bootstrap.plan_failed",
        message: "failed to build runtime bootstrap plan",
        context: { error },
      });
    } finally {
      setIsPlanningRuntimeBootstrap(false);
    }
  }

  async function authorizeRuntimeBootstrapAction(actionId: string) {
    const plan = runtimeBootstrapPlan;
    const action = plan?.actions.find((candidate) => candidate.action_id === actionId);
    if (!plan || !action) return;

    const preview = action.command_preview ?? "intervencao manual; nenhum comando automatico";
    const approved = window.confirm(
      [
        `Autorizar esta acao de setup?\n\n${action.title}`,
        `Fonte: ${action.source}`,
        `Escopo: ${action.install_scope}`,
        `Comando/acao: ${preview}`,
        action.requires_elevation
          ? "Esta acao exige uma fronteira UAC separada."
          : "Nenhuma elevacao permanente sera mantida.",
      ].join("\n"),
    );
    if (!approved) return;

    setActiveRuntimeBootstrapActionId(actionId);
    setRuntimeBootstrapResult(null);
    try {
      const result = await executeRuntimeBootstrapAction(actionId, plan.plan_hash, true);
      setRuntimeBootstrapResult(result);
      setRuntimeBootstrapPlan(result.refreshed_plan);
      appendActivity({
        level: result.status === "completed" ? "detail" : "diagnostic",
        title: `Setup: ${action.title}`,
        detail: result.message,
      });
      void logEvent({
        level: result.status === "completed" ? "info" : "warn",
        category: "runtime.bootstrap.action_finished",
        message: "runtime bootstrap action finished",
        context: {
          action_id: actionId,
          plan_hash: plan.plan_hash,
          status: result.status,
          exit_code: result.exit_code,
        },
      });
    } catch (error) {
      appendActivity({
        level: "diagnostic",
        title: `Falha no setup: ${action.title}`,
        detail: "A acao falhou fechada; gere um novo plano antes de tentar novamente.",
      });
      void logEvent({
        level: "error",
        category: "runtime.bootstrap.action_failed",
        message: "runtime bootstrap action failed",
        context: { action_id: actionId, plan_hash: plan.plan_hash, error },
      });
    } finally {
      setActiveRuntimeBootstrapActionId(null);
    }
  }

  async function controlRuntimeBootstrap(
    actionId: string,
    disposition: RuntimeBootstrapDisposition,
  ) {
    const plan = runtimeBootstrapPlan;
    if (!plan) return;
    try {
      const result = await controlRuntimeBootstrapAction(actionId, plan.plan_hash, disposition);
      appendActivity({
        level: "detail",
        title: `Acao de setup: ${result.status}`,
        detail: `${actionId} registrado como ${result.disposition}.`,
      });
      if (disposition !== "cancel") await refreshRuntimeBootstrapPlan();
    } catch (error) {
      void logEvent({
        level: "error",
        category: "runtime.bootstrap.control_failed",
        message: "runtime bootstrap action control failed",
        context: { action_id: actionId, plan_hash: plan.plan_hash, disposition, error },
      });
    }
  }

  async function verifyAgentsNow() {
    try {
      const preflight = await dependencyPreflight();
      setBootstrapRows(preflight.checks);
      const byLabel = new Map(preflight.checks.map((check) => [check.label, check]));
      setAgentCards((current) =>
        current.map((agent) => {
          const check = byLabel.get(`${agent.name} CLI`);
          if (!check) return agent;
          return {
            ...agent,
            state: check.tone === "ok" ? "ready" : check.tone === "warn" ? "evidence" : "blocked",
            note: check.value,
          };
        }),
      );
      appendActivity({
        level: "detail",
        title: "Agentes verificados",
        detail: preflight.checks
          .filter((check) => check.label.endsWith("CLI"))
          .map((check) => `${check.label}: ${check.tone}`)
          .join("; "),
      });
      void logEvent({
        level: "info",
        category: "agents.preflight.completed",
        message: "operator verified local agent CLIs",
        context: {
          checks: preflight.checks.map((check) => ({ label: check.label, tone: check.tone })),
        },
      });
    } catch (error) {
      appendActivity({
        level: "diagnostic",
        title: "Falha ao verificar agentes",
        detail: "Consulte o log desta execucao para o erro completo.",
      });
      void logEvent({
        level: "error",
        category: "agents.preflight.failed",
        message: "failed to verify local agent CLIs",
        context: { error },
      });
    }
  }

  async function revalidateRuntime() {
    appendActivity({
      level: "detail",
      title: "Revalidacao iniciada",
      detail: "Conferindo dependencias, configuracoes locais e chaves carregadas.",
    });
    await Promise.all([
      loadBootstrapConfig(),
      loadAiProviderConfig(),
      verifyAgentsNow(),
      refreshRuntimeBootstrapPlan(),
    ]);
  }

  async function openSessionLedger() {
    if (!lastSessionMinutesPath) {
      appendActivity({
        level: "summary",
        title: "Ata indisponivel",
        detail: "Ainda nao ha ata criada nesta sessao do app.",
      });
      return;
    }

    try {
      const openedPath = await openDataFile(lastSessionMinutesPath);
      appendActivity({
        level: "detail",
        title: "Ata aberta",
        detail: openedPath,
      });
      void logEvent({
        level: "info",
        category: "session.ledger.opened",
        message: "operator opened session ledger file",
        context: { path: openedPath },
      });
    } catch (error) {
      appendActivity({
        level: "diagnostic",
        title: "Falha ao abrir ata",
        detail: "O arquivo nao foi aberto; consulte o log desta execucao.",
      });
      void logEvent({
        level: "error",
        category: "session.ledger.open_failed",
        message: "failed to open session ledger file",
        context: { path: lastSessionMinutesPath, error },
      });
    }
  }

  async function auditEvidenceNow() {
    const sourceText = [editorialPrompt, protocolText, mainSiteHtml].join("\n\n");
    const citationSourceText = mainSiteHtml.trim() || editorialPrompt.trim();
    const pinnedProtocolHash = /^[a-f0-9]{64}$/i.test(protocol.hash) ? protocol.hash : null;
    setIsAuditingEvidence(true);
    setLinkAuditRows([]);
    setCitationAuditResult(null);
    setEvidenceRows((current) =>
      current.map((row) =>
        row.label === "Links"
          ? { ...row, value: "verificando links", tone: "info" }
          : row.label === "ABNT"
            ? { ...row, value: "auditando citacoes", tone: "info" }
            : row,
      ),
    );

    let citationAuditPromise: Promise<CitationAuditResult>;
    try {
      const citationManifests = citationManifestsFromAttachments(promptAttachments);
      citationAuditPromise = auditAbntCitations({
        text: citationSourceText,
        protocol_hash: pinnedProtocolHash,
        manifest: citationManifests.current,
        previous_manifest: citationManifests.previous,
      });
    } catch (error) {
      citationAuditPromise = Promise.reject(error);
      void logEvent({
        level: "error",
        category: "citation.manifest.invalid",
        message: "citation manifest attachment could not be parsed",
        context: { error },
      });
    }
    const [linkOutcome, citationOutcome] = await Promise.allSettled([
      auditLinks(sourceText),
      citationAuditPromise,
    ]);

    if (linkOutcome.status === "fulfilled") {
      const result = linkOutcome.value;
      const failedLinkLabel =
        result.failed === 1
          ? "1 link com problema"
          : `${result.failed.toLocaleString("pt-BR")} links com problema`;
      const pendingReviewLabel =
        result.pending_review === 1
          ? "1 link aguardando revisao editorial"
          : `${result.pending_review.toLocaleString("pt-BR")} links aguardando revisao editorial`;
      setLinkAuditRows(result.rows);
      setEvidenceRows((current) =>
        current.map((row) => {
          if (row.label !== "Links") return row;
          if (result.urls_found === 0) {
            return { ...row, value: "nenhum link encontrado", tone: "idle" };
          }
          if (result.failed > 0) {
            return {
              ...row,
              value: failedLinkLabel,
              tone: "warn",
            };
          }
          if (result.pending_review > 0) {
            return {
              ...row,
              value: pendingReviewLabel,
              tone: "warn",
            };
          }
          return {
            ...row,
            value: `${result.ok.toLocaleString("pt-BR")} links acessiveis`,
            tone: "ok",
          };
        }),
      );
      appendActivity({
        level: "detail",
        title: "Links auditados",
        detail:
          result.urls_found === 0
            ? "Nenhum link foi encontrado no prompt, protocolo ou texto em edicao."
            : `${result.ok.toLocaleString("pt-BR")} aceitos; ${pendingReviewLabel}; ${failedLinkLabel}.`,
      });
      void logEvent({
        level: result.failed > 0 || result.pending_review > 0 ? "warn" : "info",
        category: "evidence.audit.completed",
        message: "link evidence audit completed",
        context: {
          urls_found: result.urls_found,
          checked: result.checked,
          ok: result.ok,
          failed: result.failed,
          pending_review: result.pending_review,
          blocked: result.blocked,
          rows: result.rows.map((row) => ({
            url: row.url,
            tone: row.tone,
            status: row.status,
            invalidity: row.invalidity,
          })),
        },
      });
    } else {
      setLinkAuditRows([]);
      setEvidenceRows((current) =>
        current.map((row) =>
          row.label === "Links" ? { ...row, value: "falha na auditoria", tone: "danger" } : row,
        ),
      );
      void logEvent({
        level: "error",
        category: "evidence.audit.failed",
        message: "link evidence audit failed",
        context: { error: linkOutcome.reason },
      });
    }

    if (citationOutcome.status === "fulfilled") {
      const result = citationOutcome.value;
      const needsEvidence = result.blockers.filter((blocker) => blocker.needs_evidence).length;
      setCitationAuditResult(result);
      setEvidenceRows((current) =>
        current.map((row) => {
          if (row.label !== "ABNT") return row;
          if (result.maestro_peer_status === "ready" && result.blockers.length === 0) {
            return { ...row, value: "auditoria mecanica sem blocker", tone: "ok" };
          }
          if (result.maestro_peer_status === "ready") {
            return { ...row, value: "READY inconsistente com blockers", tone: "danger" };
          }
          if (result.maestro_peer_status === "needs_evidence") {
            return {
              ...row,
              value: `${needsEvidence.toLocaleString("pt-BR")} pendencia(s) de evidencia`,
              tone: "warn",
            };
          }
          return {
            ...row,
            value: `${result.blockers.length.toLocaleString("pt-BR")} blocker(s) mecanico(s)`,
            tone: "danger",
          };
        }),
      );
      appendActivity({
        level: result.maestro_peer_status === "ready" ? "detail" : "diagnostic",
        title: `MaestroPeer: ${result.maestro_peer_status.toUpperCase()}`,
        detail: `${result.citations.length.toLocaleString("pt-BR")} citacao(oes), ${result.normalized_references.length.toLocaleString("pt-BR")} referencia(s) normalizada(s), ${result.blockers.length.toLocaleString("pt-BR")} blocker(s). Veredito mecanico; nao equivale a aprovacao por consenso de IA.`,
      });
      void logEvent({
        level: result.maestro_peer_status === "ready" ? "info" : "warn",
        category: "citation.audit.completed",
        message: "deterministic ABNT citation audit completed",
        context: {
          audit_id: result.audit_id,
          protocol_hash: result.protocol_hash,
          maestro_peer_status: result.maestro_peer_status,
          citations: result.citations.length,
          normalized_references: result.normalized_references.length,
          blockers: result.blockers.length,
          needs_evidence: needsEvidence,
        },
      });
    } else {
      setCitationAuditResult(null);
      setEvidenceRows((current) =>
        current.map((row) =>
          row.label === "ABNT" ? { ...row, value: "falha na auditoria", tone: "danger" } : row,
        ),
      );
      void logEvent({
        level: "error",
        category: "citation.audit.failed",
        message: "deterministic ABNT citation audit failed",
        context: { error: citationOutcome.reason },
      });
    }

    setIsAuditingEvidence(false);
  }

  function createRunId() {
    return `run-${new Date().toISOString().replace(/[:.]/g, "-")}`;
  }

  function buildBootstrapConfig(nextMode = credentialStorageMode): BootstrapConfig {
    return {
      schema_version: 1,
      credential_storage_mode: nextMode,
      cloudflare_account_id:
        cloudflareAccountId.trim() || cloudflareEnvSnapshot?.account_id || null,
      cloudflare_api_token_source: cloudflareTokenSource,
      cloudflare_api_token_env_var: cloudflareTokenEnvVar.trim() || "MAESTRO_CLOUDFLARE_API_TOKEN",
      cloudflare_persistence_database: "maestro_db",
      cloudflare_secret_store: "maestro",
      windows_env_prefix: "MAESTRO_",
      updated_at: new Date().toISOString(),
    };
  }

  async function loadBootstrapConfig() {
    try {
      const [config, envSnapshot] = await Promise.all([
        readBootstrapConfig(),
        readCloudflareEnvSnapshot(),
      ]);

      setBootstrapRows(
        initialBootstrapChecks.map((row) => ({
          ...row,
          value: row.label === "WebView2" ? "ativo pelo runtime Tauri" : "verificando",
          tone: row.label === "WebView2" ? "ok" : row.tone,
        })),
      );
      setCredentialStorageMode(config.credential_storage_mode);
      setCloudflareTokenSource(
        envSnapshot.api_token_present ? "windows_env" : config.cloudflare_api_token_source,
      );
      setCloudflareTokenEnvVar(
        envSnapshot.api_token_env_var ?? config.cloudflare_api_token_env_var,
      );
      setCloudflareEnvSnapshot(envSnapshot);
      if (!cloudflareAccountId.trim() && (envSnapshot.account_id || config.cloudflare_account_id)) {
        setCloudflareAccountId(envSnapshot.account_id ?? config.cloudflare_account_id ?? "");
      }
      setBootstrapConfigStatus(
        `bootstrap.json carregado; token Cloudflare ${
          envSnapshot.api_token_present
            ? `detectado em ${envSnapshot.api_token_env_var} (${envSnapshot.api_token_env_scope ?? "process"})`
            : "nao detectado em env var"
        }`,
      );
      void logEvent({
        level: "info",
        category: "bootstrap.config.loaded",
        message: "bootstrap configuration and Cloudflare environment snapshot loaded",
        context: {
          credential_storage_mode: config.credential_storage_mode,
          cloudflare_account_id_source: envSnapshot.account_id_env_var
            ? "windows_env"
            : config.cloudflare_account_id
              ? "bootstrap_json"
              : "missing",
          cloudflare_account_id_env_scope: envSnapshot.account_id_env_scope ?? "missing",
          cloudflare_api_token_source: envSnapshot.api_token_present
            ? "windows_env"
            : config.cloudflare_api_token_source,
          cloudflare_api_token_env_var:
            envSnapshot.api_token_env_var ?? config.cloudflare_api_token_env_var,
          cloudflare_api_token_env_scope: envSnapshot.api_token_env_scope ?? "missing",
          cloudflare_api_token_present: envSnapshot.api_token_present,
        },
      });
      void dependencyPreflight()
        .then((preflight) => {
          setBootstrapRows(preflight.checks);
          void logEvent({
            level: "info",
            category: "bootstrap.dependency_preflight.completed",
            message: "background dependency preflight completed",
            context: {
              checks: preflight.checks.map((check) => ({
                label: check.label,
                tone: check.tone,
              })),
            },
          });
        })
        .catch((error) => {
          setBootstrapRows((current) =>
            current.map((row) =>
              row.label === "WebView2"
                ? row
                : { ...row, value: "falha na verificacao; consulte diagnostico", tone: "warn" },
            ),
          );
          void logEvent({
            level: "warn",
            category: "bootstrap.dependency_preflight.failed",
            message: "background dependency preflight failed",
            context: { error },
          });
        });
    } catch (error) {
      setBootstrapConfigStatus("falha ao carregar bootstrap.json");
      void logEvent({
        level: "error",
        category: "bootstrap.config.load_failed",
        message: "failed to load bootstrap configuration",
        context: { error },
      });
    }
  }

  async function persistBootstrapConfig(nextMode = credentialStorageMode) {
    try {
      const saved = await writeBootstrapConfig(buildBootstrapConfig(nextMode));
      setBootstrapConfigStatus(`bootstrap.json salvo em ${saved.updated_at}`);
      void logEvent({
        level: "info",
        category: "bootstrap.config.saved",
        message: "bootstrap configuration saved without secrets",
        context: {
          credential_storage_mode: saved.credential_storage_mode,
          cloudflare_account_id_present: Boolean(saved.cloudflare_account_id),
          cloudflare_api_token_source: saved.cloudflare_api_token_source,
          cloudflare_api_token_env_var: saved.cloudflare_api_token_env_var,
        },
      });
    } catch (error) {
      setBootstrapConfigStatus("falha ao salvar bootstrap.json");
      void logEvent({
        level: "error",
        category: "bootstrap.config.save_failed",
        message: "failed to save bootstrap configuration",
        context: { error },
      });
    }
  }

  function buildAiProviderConfig(nextProviderMode = providerMode): AiProviderConfig {
    return {
      schema_version: 1,
      provider_mode: nextProviderMode,
      credential_storage_mode: credentialStorageMode,
      openai_api_key: aiCredentials.openai.trim() || null,
      anthropic_api_key: aiCredentials.anthropic.trim() || null,
      gemini_api_key: aiCredentials.gemini.trim() || null,
      deepseek_api_key: aiCredentials.deepseek.trim() || null,
      grok_api_key: aiCredentials.grok.trim() || null,
      perplexity_api_key: aiCredentials.perplexity.trim() || null,
      openai_api_key_remote: false,
      anthropic_api_key_remote: false,
      gemini_api_key_remote: false,
      deepseek_api_key_remote: false,
      grok_api_key_remote: false,
      perplexity_api_key_remote: false,
      openai_input_usd_per_million: parseOptionalPositiveNumber(
        providerInputUsdPerMillion.openai,
        "Tarifa OpenAI de entrada",
        10000,
      ),
      openai_output_usd_per_million: parseOptionalPositiveNumber(
        providerOutputUsdPerMillion.openai,
        "Tarifa OpenAI de saida",
        10000,
      ),
      anthropic_input_usd_per_million: parseOptionalPositiveNumber(
        providerInputUsdPerMillion.anthropic,
        "Tarifa Anthropic de entrada",
        10000,
      ),
      anthropic_output_usd_per_million: parseOptionalPositiveNumber(
        providerOutputUsdPerMillion.anthropic,
        "Tarifa Anthropic de saida",
        10000,
      ),
      gemini_input_usd_per_million: parseOptionalPositiveNumber(
        providerInputUsdPerMillion.gemini,
        "Tarifa Gemini de entrada",
        10000,
      ),
      gemini_output_usd_per_million: parseOptionalPositiveNumber(
        providerOutputUsdPerMillion.gemini,
        "Tarifa Gemini de saida",
        10000,
      ),
      deepseek_input_usd_per_million: parseOptionalPositiveNumber(
        providerInputUsdPerMillion.deepseek,
        "Tarifa DeepSeek de entrada",
        10000,
      ),
      deepseek_output_usd_per_million: parseOptionalPositiveNumber(
        providerOutputUsdPerMillion.deepseek,
        "Tarifa DeepSeek de saida",
        10000,
      ),
      grok_input_usd_per_million: parseOptionalPositiveNumber(
        providerInputUsdPerMillion.grok,
        "Tarifa Grok de entrada",
        10000,
      ),
      grok_output_usd_per_million: parseOptionalPositiveNumber(
        providerOutputUsdPerMillion.grok,
        "Tarifa Grok de saida",
        10000,
      ),
      perplexity_input_usd_per_million: parseOptionalPositiveNumber(
        providerInputUsdPerMillion.perplexity,
        "Tarifa Perplexity de entrada",
        10000,
      ),
      perplexity_output_usd_per_million: parseOptionalPositiveNumber(
        providerOutputUsdPerMillion.perplexity,
        "Tarifa Perplexity de saida",
        10000,
      ),
      cloudflare_secret_store_id: null,
      cloudflare_secret_store_name: null,
      updated_at: new Date().toISOString(),
    };
  }

  function buildCloudflareProviderStorageRequest(): CloudflareProviderStorageRequest {
    return {
      account_id: cloudflareAccountId.trim() || cloudflareEnvSnapshot?.account_id || "",
      api_token: cloudflareApiToken.trim() || null,
      api_token_env_var:
        cloudflareTokenEnvVar.trim() ||
        cloudflareEnvSnapshot?.api_token_env_var ||
        "MAESTRO_CLOUDFLARE_API_TOKEN",
      persistence_database: "maestro_db",
      secret_store: "maestro",
    };
  }

  function aiConfigStorageLabel(mode: CredentialStorageMode) {
    if (mode === "cloudflare") return "Cloudflare D1 + Secrets Store";
    if (mode === "windows_env") return "env vars do Windows + JSON local";
    return "data/config/ai-providers.json";
  }

  async function loadAiProviderConfig() {
    try {
      const config = await readAiProviderConfig();
      setProviderMode(config.provider_mode);
      setAiCredentials({
        openai: config.openai_api_key ?? "",
        anthropic: config.anthropic_api_key ?? "",
        gemini: config.gemini_api_key ?? "",
        deepseek: config.deepseek_api_key ?? "",
        grok: config.grok_api_key ?? "",
        perplexity: config.perplexity_api_key ?? "",
      });
      applyProviderRatesFromConfig(config);
      const remoteCount = [
        config.openai_api_key_remote,
        config.anthropic_api_key_remote,
        config.gemini_api_key_remote,
        config.deepseek_api_key_remote,
        config.grok_api_key_remote,
        config.perplexity_api_key_remote,
      ].filter(Boolean).length;
      setAiConfigStatus(
        remoteCount > 0
          ? `Configuracao carregada de ${aiConfigStorageLabel(
              config.credential_storage_mode,
            )}; ${remoteCount.toLocaleString("pt-BR")} referencia(s) remota(s) no Cloudflare`
          : `Configuracao carregada de ${aiConfigStorageLabel(config.credential_storage_mode)}`,
      );
      void logEvent({
        level: "info",
        category: "settings.ai_provider.config_loaded",
        message: "AI provider configuration loaded",
        context: {
          provider_mode: config.provider_mode,
          credential_storage_mode: config.credential_storage_mode,
          openai_key_present: Boolean(config.openai_api_key),
          anthropic_key_present: Boolean(config.anthropic_api_key),
          gemini_key_present: Boolean(config.gemini_api_key),
          deepseek_key_present: Boolean(config.deepseek_api_key),
          grok_key_present: Boolean(config.grok_api_key),
          perplexity_key_present: Boolean(config.perplexity_api_key),
          openai_rate_input_configured: config.openai_input_usd_per_million != null,
          openai_rate_output_configured: config.openai_output_usd_per_million != null,
          anthropic_rate_input_configured: config.anthropic_input_usd_per_million != null,
          anthropic_rate_output_configured: config.anthropic_output_usd_per_million != null,
          gemini_rate_input_configured: config.gemini_input_usd_per_million != null,
          gemini_rate_output_configured: config.gemini_output_usd_per_million != null,
          deepseek_cost_input_configured: config.deepseek_input_usd_per_million != null,
          deepseek_cost_output_configured: config.deepseek_output_usd_per_million != null,
          grok_cost_input_configured: config.grok_input_usd_per_million != null,
          grok_cost_output_configured: config.grok_output_usd_per_million != null,
          perplexity_cost_input_configured: config.perplexity_input_usd_per_million != null,
          perplexity_cost_output_configured: config.perplexity_output_usd_per_million != null,
          openai_remote_present: config.openai_api_key_remote,
          anthropic_remote_present: config.anthropic_api_key_remote,
          gemini_remote_present: config.gemini_api_key_remote,
          deepseek_remote_present: config.deepseek_api_key_remote,
          grok_remote_present: config.grok_api_key_remote,
          perplexity_remote_present: config.perplexity_api_key_remote,
        },
      });
    } catch (error) {
      setAiConfigStatus("Falha ao carregar configuracao das APIs");
      void logEvent({
        level: "error",
        category: "settings.ai_provider.config_load_failed",
        message: "failed to load AI provider configuration",
        context: { error },
      });
    }
  }

  async function saveAiProviderConfig(nextProviderMode = providerMode) {
    setIsSavingAiConfig(true);
    try {
      const saved = await writeAiProviderConfig(
        buildAiProviderConfig(nextProviderMode),
        credentialStorageMode === "cloudflare" ? buildCloudflareProviderStorageRequest() : null,
      );
      setProviderMode(saved.provider_mode);
      setAiCredentials({
        openai: saved.openai_api_key ?? "",
        anthropic: saved.anthropic_api_key ?? "",
        gemini: saved.gemini_api_key ?? "",
        deepseek: saved.deepseek_api_key ?? "",
        grok: saved.grok_api_key ?? "",
        perplexity: saved.perplexity_api_key ?? "",
      });
      applyProviderRatesFromConfig(saved);
      const storageLabel = aiConfigStorageLabel(saved.credential_storage_mode);
      setAiConfigStatus(
        `Salvo em ${storageLabel} as ${formatBrazilDateTime(new Date(saved.updated_at))}`,
      );
      appendActivity({
        level: "detail",
        title: "Configuracao salva",
        detail:
          saved.credential_storage_mode === "cloudflare"
            ? "As chaves informadas foram enviadas ao Cloudflare Secrets Store; o JSON local guarda apenas o marcador do modo remoto."
            : "As chaves de API foram salvas conforme o modo de persistencia selecionado.",
      });
      void logEvent({
        level: "info",
        category: "settings.ai_provider.config_saved",
        message: "AI provider configuration saved",
        context: {
          provider_mode: saved.provider_mode,
          credential_storage_mode: saved.credential_storage_mode,
          openai_key_present: Boolean(saved.openai_api_key),
          anthropic_key_present: Boolean(saved.anthropic_api_key),
          gemini_key_present: Boolean(saved.gemini_api_key),
          deepseek_key_present: Boolean(saved.deepseek_api_key),
          grok_key_present: Boolean(saved.grok_api_key),
          perplexity_key_present: Boolean(saved.perplexity_api_key),
          openai_rate_input_configured: saved.openai_input_usd_per_million != null,
          openai_rate_output_configured: saved.openai_output_usd_per_million != null,
          anthropic_rate_input_configured: saved.anthropic_input_usd_per_million != null,
          anthropic_rate_output_configured: saved.anthropic_output_usd_per_million != null,
          gemini_rate_input_configured: saved.gemini_input_usd_per_million != null,
          gemini_rate_output_configured: saved.gemini_output_usd_per_million != null,
          deepseek_cost_input_configured: saved.deepseek_input_usd_per_million != null,
          deepseek_cost_output_configured: saved.deepseek_output_usd_per_million != null,
          grok_cost_input_configured: saved.grok_input_usd_per_million != null,
          grok_cost_output_configured: saved.grok_output_usd_per_million != null,
          perplexity_cost_input_configured: saved.perplexity_input_usd_per_million != null,
          perplexity_cost_output_configured: saved.perplexity_output_usd_per_million != null,
          openai_remote_present: saved.openai_api_key_remote,
          anthropic_remote_present: saved.anthropic_api_key_remote,
          gemini_remote_present: saved.gemini_api_key_remote,
          deepseek_remote_present: saved.deepseek_api_key_remote,
          grok_remote_present: saved.grok_api_key_remote,
          perplexity_remote_present: saved.perplexity_api_key_remote,
        },
      });
      return saved;
    } catch (error) {
      setAiConfigStatus(
        error instanceof Error ? error.message : "Falha ao salvar configuracao das APIs",
      );
      void logEvent({
        level: "error",
        category: "settings.ai_provider.config_save_failed",
        message: "failed to save AI provider configuration",
        context: { error },
      });
      return null;
    } finally {
      setIsSavingAiConfig(false);
    }
  }

  function chooseVerbosity(nextVerbosity: VerbosityMode) {
    setVerbosity(nextVerbosity);
    void logEvent({
      level: "info",
      category: "ui.verbosity.changed",
      message: "operator changed interface verbosity",
      context: { verbosity: nextVerbosity, session_name: sessionName },
    });
  }

  function chooseSection(nextSection: ActiveSection) {
    setActiveSection(nextSection);
    void logEvent({
      level: "info",
      category: "ui.navigation.changed",
      message: "operator changed active Maestro section",
      context: { active_section: nextSection, session_name: sessionName },
    });
  }

  function chooseSettingsTab(nextTab: SettingsTab) {
    setActiveSettingsTab(nextTab);
    void logEvent({
      level: "info",
      category: "ui.settings.navigation.changed",
      message: "operator changed active Maestro settings tab",
      context: { active_settings_tab: nextTab, session_name: sessionName },
    });
  }

  async function importProtocol(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    if (!file) return;
    const text = await file.text();
    const nextProtocol = {
      name: file.name,
      size: file.size,
      lines: text.split(/\r?\n/).length,
      hash: await sha256(text),
    };
    setProtocol(nextProtocol);
    setProtocolText(text);
    void logEvent({
      level: "info",
      category: "protocol.imported",
      message: "operator imported editorial protocol",
      context: nextProtocol,
    });
    event.target.value = "";
  }

  function formatSessionActivity(session: ResumableSessionInfo) {
    if (!session.last_activity_unix) return "sem data registrada";
    return formatBrazilDateTime(session.last_activity_unix * 1000);
  }

  function resumeProtocolOptions(useLoadedProtocol: boolean) {
    if (!useLoadedProtocol || !hasLoadedProtocolForResume) {
      return { nextRound: undefined };
    }

    return {
      protocolName: protocol.name,
      protocolText,
      protocolHash: protocol.hash,
      nextRound: undefined,
    };
  }

  async function requestResumeSession() {
    setIsResumeLoading(true);
    setOperation({
      title: "Buscando sessoes",
      progress: 16,
      current: "Verificando sessoes interrompidas na pasta de dados.",
      eta: "aguarde",
      status: "preparing",
    });

    try {
      const sessions = await listResumableSessions();
      setResumeCandidates(sessions);
      setUseLoadedProtocolForResume(hasLoadedProtocolForResume);

      void logEvent({
        level: "info",
        category: "session.resume.requested",
        message: "operator requested resumable session list",
        context: {
          count: sessions.length,
          loaded_protocol_available: hasLoadedProtocolForResume,
          protocol_name: hasLoadedProtocolForResume ? protocol.name : null,
        },
      });

      if (sessions.length === 0) {
        setOperation({
          title: "Nenhuma sessao para retomar",
          progress: 0,
          current: "Nao encontrei sessoes interrompidas na pasta de dados.",
          eta: "inicie uma nova sessao quando quiser",
          status: "idle",
        });
        appendActivity({
          level: "summary",
          title: "Nada para retomar",
          detail: "A pasta de sessoes nao possui trabalhos interrompidos disponiveis.",
        });
        return;
      }

      if (sessions.length === 1) {
        const session = sessions[0];
        if (session) {
          await startResumeSession(session, hasLoadedProtocolForResume);
        }
        return;
      }

      setShowResumePicker(true);
      setOperation({
        title: "Escolha a sessao",
        progress: 28,
        current: `${sessions.length.toLocaleString("pt-BR")} sessoes interrompidas encontradas.`,
        eta: "selecione qual trabalho continuar",
        status: "paused",
      });
    } catch (error) {
      setOperation({
        title: "Retomada indisponivel",
        progress: 0,
        current: "Nao foi possivel ler as sessoes salvas.",
        eta: "consulte diagnostico",
        status: "blocked",
      });
      void logEvent({
        level: "error",
        category: "session.resume.list_failed",
        message: "failed to list resumable sessions",
        context: { error },
      });
    } finally {
      setIsResumeLoading(false);
    }
  }

  async function startResumeSession(session: ResumableSessionInfo, useLoadedProtocol: boolean) {
    setShowResumePicker(false);
    sessionRunIdRef.current = session.run_id;
    setSessionRunId(session.run_id);
    setActiveAgentNow(null);
    setSessionName(session.session_name);
    const protocolOverride = resumeProtocolOptions(useLoadedProtocol);

    // B21 fix (v0.5.1, operator-reported "maestro-app importa os peers
    // anteriormente configurados, não respeitando novas configurações"):
    // resume MUST honor the operator's CURRENT React state (peer toggles +
    // initial-agent picker + caps), NOT the saved session contract. The
    // saved_active_agents/saved_initial_agent fields stay in
    // ResumableSessionInfo for the picker UI to display informationally
    // (so the operator can see what the session was running with), but
    // they are NEVER auto-applied to React state and NEVER injected into
    // resumeRunOptions. This mirrors the v0.3.42 B20 fix for caps:
    // request is source of truth; saved_contract is reference only.
    //
    // Replaces the v0.3.18 B17 behavior (auto-pre-populate from saved on
    // resume) which was casca vazia in disguise — UI showed operator's
    // selection but resume silently overrode with saved values.
    let resumeRunOptions: SessionRunOptions;
    try {
      resumeRunOptions = currentSessionRunOptions();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setOperation({
        title: "Retomada bloqueada",
        progress: 0,
        current: message,
        eta: "Ajuste a configuracao de peers e tente novamente.",
        status: "blocked",
      });
      void logEvent({
        level: "error",
        category: "session.resume.run_options_invalid",
        message: "resume aborted because UI peers/caps state is invalid",
        context: { run_id: session.run_id, error: message },
      });
      return;
    }
    void logEvent({
      level: "info",
      category: "session.resume.contract_applied",
      message: "resume honoring current UI state; saved contract values are reference-only",
      context: {
        run_id: session.run_id,
        // What the saved session contract had — informational only, NOT applied.
        saved_active_agents: session.saved_active_agents,
        saved_initial_agent: session.saved_initial_agent,
        // What the resume actually uses — comes from the current React state.
        requested_active_agents: resumeRunOptions.activeAgents,
        requested_initial_agent: initialAgent,
        requested_max_session_cost_usd: resumeRunOptions.maxSessionCostUsd,
        requested_max_session_minutes: resumeRunOptions.maxSessionMinutes,
      },
    });
    setOperation({
      title: "Retomando sessao editorial",
      progress: 32,
      current: `Continuando a partir da rodada ${session.next_round.toLocaleString("pt-BR")}.`,
      eta: `Ultima atividade: ${formatSessionActivity(session)}`,
      status: "preparing",
    });
    setPhaseItems([
      {
        label: "Protocolo",
        detail: useLoadedProtocol && hasLoadedProtocolForResume ? "atualizado" : "salvo",
        state: "done",
      },
      { label: "Verificacoes", detail: "concluidas", state: "done" },
      { label: "Agentes", detail: "preparando continuidade", state: "active" },
      { label: "Entrega", detail: "aguardando unanimidade", state: "waiting" },
    ]);
    setDiscussionItems((current) => [
      {
        round: session.next_round.toString().padStart(3, "0"),
        status: "Retomada",
        note:
          useLoadedProtocol && hasLoadedProtocolForResume
            ? `Sessao retomada com o protocolo carregado: ${protocol.name}.`
            : "Sessao retomada com o protocolo salvo na pasta da sessao.",
      },
      ...current,
    ]);
    appendActivity({
      level: "summary",
      title: "Retomada iniciada",
      detail:
        useLoadedProtocol && hasLoadedProtocolForResume
          ? `Rodada ${session.next_round.toLocaleString("pt-BR")} com protocolo atualizado.`
          : `Rodada ${session.next_round.toLocaleString("pt-BR")} com protocolo salvo.`,
    });
    void logEvent({
      level: "info",
      category: "session.resume.selected",
      message: "operator selected session to resume",
      context: {
        run_id: session.run_id,
        session_name: session.session_name,
        next_round: session.next_round,
        use_loaded_protocol: useLoadedProtocol && hasLoadedProtocolForResume,
        loaded_protocol_name: hasLoadedProtocolForResume ? protocol.name : null,
      },
    });

    const resumeInitialAgent: InitialAgentKey = resumeRunOptions.activeAgents.includes(initialAgent)
      ? initialAgent
      : (resumeRunOptions.activeAgents[0] ?? initialAgent);

    await runRealEditorialSession(
      session.run_id,
      "",
      {
        ...protocolOverride,
        nextRound: session.next_round,
      },
      resumeInitialAgent,
      resumeRunOptions,
    );
  }

  function toggleActiveAgent(agent: InitialAgentKey) {
    setActiveAgents((current) => {
      if (current.includes(agent)) {
        if (current.length === 1) return current;
        return current.filter((item) => item !== agent);
      }
      return [...current, agent]
        .filter((item, index, items) => items.indexOf(item) === index)
        .slice(0, 6);
    });
  }

  function parseOptionalPositiveNumber(value: string, label: string, maxValue?: number) {
    const trimmed = value.trim().replace(",", ".");
    if (!trimmed) return null;
    const parsed = Number(trimmed);
    if (!Number.isFinite(parsed) || parsed <= 0) {
      throw new Error(`${label} precisa ser um numero positivo ou ficar em branco.`);
    }
    if (maxValue != null && parsed > maxValue) {
      throw new Error(`${label} precisa ser menor ou igual a ${maxValue.toLocaleString("pt-BR")}.`);
    }
    return parsed;
  }

  function parseOptionalPositiveInteger(value: string, label: string) {
    const parsed = parseOptionalPositiveNumber(value, label);
    if (parsed == null) return null;
    if (!Number.isInteger(parsed)) {
      throw new Error(`${label} precisa ser um numero inteiro de minutos ou ficar em branco.`);
    }
    return parsed;
  }

  function parseSessionLinks() {
    return sessionLinks
      .split(/\r?\n|,/)
      .map((link) => link.trim())
      .filter(Boolean);
  }

  function currentSessionRunOptions(): SessionRunOptions {
    if (activeAgents.length < 1 || activeAgents.length > 6) {
      throw new Error("Selecione de 1 a 6 peers para a sessao.");
    }
    if (!activeAgents.includes(initialAgent)) {
      throw new Error("O agente da primeira versao precisa estar entre os peers ativos.");
    }
    const apiAgentLabels = activeAgents
      .filter((agent) => agentUsesApi(agent))
      .map((agent) => initialAgentOptions.find((option) => option.key === agent)?.label ?? agent);
    const maxCostUsd = parseOptionalPositiveNumber(maxSessionCostUsd, "Limite de custo");
    if (apiAgentLabels.length > 0 && maxCostUsd == null) {
      throw new Error(
        `Defina um limite de custo em USD para usar peers via API (${apiAgentLabels.join(", ")}). Chamadas pagas nao rodam sem teto definido pelo usuario.`,
      );
    }
    const missingRateLabels = activeAgents
      .filter((agent) => agentUsesApi(agent))
      .filter((agent) => {
        const provider = providerForAgent[agent];
        parseOptionalPositiveNumber(
          providerInputUsdPerMillion[provider],
          `Tarifa ${provider} de entrada`,
          10000,
        );
        parseOptionalPositiveNumber(
          providerOutputUsdPerMillion[provider],
          `Tarifa ${provider} de saida`,
          10000,
        );
        return !providerRatesConfigured(provider);
      })
      .map((agent) => initialAgentOptions.find((option) => option.key === agent)?.label ?? agent);
    if (missingRateLabels.length > 0) {
      throw new Error(
        `Configure as tarifas de entrada e saida em Configuracoes > Agentes via API > Tabela de tarifas para: ${missingRateLabels.join(", ")}.`,
      );
    }
    return {
      activeAgents,
      maxSessionCostUsd: maxCostUsd,
      maxSessionMinutes: parseOptionalPositiveInteger(maxSessionMinutes, "Limite de tempo"),
      attachments: promptAttachments,
      links: parseSessionLinks(),
    };
  }

  async function handlePromptAttachments(event: ChangeEvent<HTMLInputElement>) {
    const files = Array.from(event.target.files ?? []);
    event.target.value = "";
    if (files.length === 0) return;
    const nextTotal = attachmentTotalBytes + files.reduce((total, file) => total + file.size, 0);
    if (promptAttachments.length + files.length > attachmentLimits.maxFiles) {
      appendActivity({
        level: "summary",
        title: "Anexos recusados",
        detail: `Limite de ${attachmentLimits.maxFiles} arquivos por sessao.`,
      });
      return;
    }
    if (
      files.some((file) => file.size > attachmentLimits.maxFileBytes) ||
      nextTotal > attachmentLimits.maxTotalBytes
    ) {
      appendActivity({
        level: "summary",
        title: "Anexos recusados",
        detail: "Use arquivos de ate 25 MiB cada e ate 75 MiB no total.",
      });
      return;
    }
    const payloads = await Promise.all(files.map(fileToAttachmentPayload));
    setPromptAttachments((current) => [...current, ...payloads]);
  }

  async function fileToAttachmentPayload(file: File): Promise<PromptAttachmentPayload> {
    const bytes = await file.arrayBuffer();
    const view = new Uint8Array(bytes);
    let binary = "";
    const chunkSize = 0x8000;
    for (let index = 0; index < view.length; index += chunkSize) {
      binary += String.fromCharCode(...view.subarray(index, index + chunkSize));
    }
    return {
      name: file.name,
      media_type: file.type || null,
      size_bytes: file.size,
      data_base64: btoa(binary),
    };
  }

  function removePromptAttachment(name: string, sizeBytes: number) {
    setPromptAttachments((current) =>
      current.filter((item) => !(item.name === name && item.size_bytes === sizeBytes)),
    );
  }

  function startEditorialSession() {
    const promptText = editorialPrompt.trim();
    const runId = createRunId();

    if (!promptText) {
      setOperation({
        title: "Prompt ausente",
        progress: 0,
        current: "Escreva uma solicitacao antes de iniciar a sessao.",
        eta: "aguardando entrada",
        status: "blocked",
      });
      appendActivity({
        level: "summary",
        title: "Prompt vazio bloqueado",
        detail: "Nenhum agente sera acionado sem uma solicitacao editorial concreta.",
      });
      void logEvent({
        level: "warn",
        category: "session.prompt.rejected",
        message: "operator tried to start an editorial session without a prompt",
        context: { session_name: sessionName },
      });
      return;
    }

    if (protocolText.trim().length < 100) {
      setOperation({
        title: "Protocolo integral ausente",
        progress: 0,
        current: "Importe o arquivo Markdown integral do protocolo antes de iniciar a sessao.",
        eta: "aguardando protocolo",
        status: "blocked",
      });
      appendActivity({
        level: "summary",
        title: "Protocolo ausente",
        detail:
          "A sessao foi bloqueada porque o texto integral do protocolo ainda nao foi carregado ou e curto demais.",
      });
      void logEvent({
        level: "warn",
        category: "session.protocol.rejected",
        message: "operator tried to start an editorial session without full protocol text loaded",
        context: {
          session_name: sessionName,
          protocol_name: protocol.name,
          protocol_lines: protocol.lines,
          protocol_hash: protocol.hash,
        },
      });
      return;
    }

    let runOptions: SessionRunOptions;
    try {
      runOptions = currentSessionRunOptions();
    } catch (error) {
      const message = error instanceof Error ? error.message : "Controles da sessao invalidos.";
      setOperation({
        title: "Controles invalidos",
        progress: 0,
        current: message,
        eta: "ajuste peers, custo ou tempo",
        status: "blocked",
      });
      appendActivity({
        level: "summary",
        title: "Sessao bloqueada",
        detail: message,
      });
      void logEvent({
        level: "warn",
        category: "session.controls.rejected",
        message: "operator tried to start an editorial session with invalid controls",
        context: { error: message },
      });
      return;
    }

    sessionRunIdRef.current = runId;
    setSessionRunId(runId);
    setActiveAgentNow(null);
    const selectedInitialAgent = initialAgent;
    const selectedInitialAgentLabel =
      initialAgentOptions.find((option) => option.key === selectedInitialAgent)?.label ?? "Claude";
    setOperation({
      title: "Preparando sessao editorial",
      progress: 8,
      current: "Prompt recebido; fixando protocolo e abrindo ata operacional.",
      eta: runId,
      status: "preparing",
    });
    setPhaseItems([
      { label: "Protocolo", detail: "registrando", state: "active" },
      { label: "Verificacoes", detail: "aguardando protocolo", state: "waiting" },
      { label: "Agentes", detail: "nao iniciados", state: "waiting" },
      { label: "Entrega", detail: "bloqueada ate unanimidade", state: "waiting" },
    ]);
    setAgentCards(
      initialAgents.map((agent) => ({ ...agent, note: "aguardando verificacoes iniciais" })),
    );
    setEvidenceRows(
      initialEvidenceRows.map((item) => ({ ...item, value: "aguardando verificacoes" })),
    );
    setProtocolGateItems(initialProtocolReadingGates);
    setDiscussionItems([
      {
        round: "000",
        status: "Sessao criada",
        note: `Prompt recebido. ${selectedInitialAgentLabel} abrira a primeira versao; peers ativos: ${activeAgentLabels}.`,
      },
    ]);
    setActivityItems([
      {
        level: "summary",
        time: activityTimestamp(),
        title: "Prompt recebido",
        detail:
          "Sessao criada. A partir daqui, cada etapa aparecera no acompanhamento e no diagnostico.",
      },
      ...idleActivityFeed,
    ]);
    void logEvent({
      level: "info",
      category: "session.prompt.submitted",
      message: "operator submitted editorial generation prompt",
      context: {
        run_id: runId,
        session_name: sessionName,
        prompt_chars: editorialPrompt.length,
        protocol_name: protocol.name,
        protocol_lines: protocol.lines,
        protocol_chars: protocolText.length,
        required_outputs: finalArtifacts.map((artifact) => artifact.name),
        consensus_gate: "selected_editorial_agents_ready_same_round",
        initial_agent: selectedInitialAgent,
        active_agents: runOptions.activeAgents,
        max_session_cost_usd: runOptions.maxSessionCostUsd,
        max_session_minutes: runOptions.maxSessionMinutes,
        attachment_count: runOptions.attachments.length,
        link_count: runOptions.links.length,
      },
    });
    void logEvent({
      level: "info",
      category: "session.orchestration.started",
      message: "visible editorial session monitor started",
      context: {
        run_id: runId,
        provider_mode: providerMode,
        credential_storage_mode: credentialStorageMode,
        initial_agent: selectedInitialAgent,
        active_agents: runOptions.activeAgents,
      },
    });

    setOperation({
      title: "Protocolo fixado",
      progress: 22,
      current: `Protocolo ativo registrado com ${protocol.lines.toLocaleString("pt-BR")} linhas.`,
      eta: runId,
      status: "preparing",
    });
    setPhaseItems([
      { label: "Protocolo", detail: "registrado", state: "done" },
      { label: "Verificacoes", detail: "concluidas", state: "done" },
      { label: "Agentes", detail: "iniciando", state: "active" },
      { label: "Entrega", detail: "bloqueada ate unanimidade", state: "waiting" },
    ]);
    setEvidenceRows([
      { label: "DOI", value: "Aguardando", tone: "info" },
      { label: "Links", value: "Aguardando", tone: "info" },
      { label: "ABNT", value: "Aguardando", tone: "info" },
      { label: "Quarentena", value: "Aguardando", tone: "info" },
    ]);
    appendActivity({
      level: "detail",
      title: "Protocolo registrado",
      detail: `Arquivo ${protocol.name}; ${protocol.lines.toLocaleString("pt-BR")} linhas registradas.`,
    });
    void logEvent({
      level: "info",
      category: "session.protocol.pinned",
      message: "editorial protocol pinned for current visible session",
      context: {
        run_id: runId,
        protocol_name: protocol.name,
        protocol_lines: protocol.lines,
        protocol_hash: protocol.hash,
      },
    });
    void logEvent({
      level: "info",
      category: "session.preflight.completed",
      message: "local visible preflight completed",
      context: { run_id: runId },
    });
    void runRealEditorialSession(runId, promptText, undefined, selectedInitialAgent, runOptions);
  }

  async function runRealEditorialSession(
    runId: string,
    promptText: string,
    resumeOptions?: {
      protocolName?: string;
      protocolText?: string;
      protocolHash?: string;
      nextRound?: number;
    },
    selectedInitialAgent: InitialAgentKey = initialAgent,
    runOptions?: SessionRunOptions,
  ) {
    const isResume = Boolean(resumeOptions);
    const startedAt = Date.now();
    const startedAtLabel = formatBrazilDateTime(startedAt);
    const selectedInitialAgentLabel =
      initialAgentOptions.find((option) => option.key === selectedInitialAgent)?.label ?? "Claude";
    setOperation({
      title: isResume ? "Retomando sessao editorial" : "Sessao editorial em andamento",
      progress: 44,
      current: isResume
        ? `Continuando a partir da rodada ${resumeOptions?.nextRound?.toLocaleString("pt-BR") ?? "salva"}.`
        : `${selectedInitialAgentLabel} esta preparando a primeira versao; peers ativos: ${
            runOptions?.activeAgents
              .map(
                (agent) =>
                  initialAgentOptions.find((option) => option.key === agent)?.label ?? agent,
              )
              .join(", ") ?? activeAgentLabels
          }.`,
      eta: `Inicio: ${startedAtLabel}`,
      status: "running",
    });
    setIsStopRequested(false);
    setPhaseItems([
      { label: "Protocolo", detail: "registrado", state: "done" },
      { label: "Verificacoes", detail: "concluidas", state: "done" },
      { label: "Agentes", detail: "em execucao", state: "active" },
      { label: "Entrega", detail: "aguardando unanimidade", state: "waiting" },
    ]);
    setAgentCards([
      ...initialAgentOptions.map((option) => ({
        name: option.label,
        cli: option.key,
        state:
          runOptions && !runOptions.activeAgents.includes(option.key)
            ? ("blocked" as AgentState)
            : ("running" as AgentState),
        note:
          runOptions && !runOptions.activeAgents.includes(option.key)
            ? "fora desta sessao"
            : option.key === selectedInitialAgent
              ? "primeira versao e ajustes em andamento"
              : "leitura e revisao em andamento",
      })),
      { name: "Maestro", cli: "motor local", state: "running", note: "acompanhando a unanimidade" },
    ]);
    appendActivity({
      level: "diagnostic",
      title: "Sessao iniciada",
      detail: "O Maestro esta acompanhando os agentes e registrando os arquivos da rodada.",
    });
    void logEvent({
      level: "info",
      category: "session.editorial.requested",
      message: "frontend requested real editorial session",
      context: {
        run_id: runId,
        session_name: sessionName,
        prompt_chars: isResume && promptText.length === 0 ? null : promptText.length,
        prompt_source:
          isResume && promptText.length === 0 ? "saved_session_prompt" : "current_editor_prompt",
        resume_mode: isResume,
        resume_next_round: resumeOptions?.nextRound ?? null,
        resume_protocol_override: Boolean(resumeOptions?.protocolText),
        protocol_name: isResume
          ? (resumeOptions?.protocolName ?? "protocolo salvo na sessao")
          : protocol.name,
        protocol_lines: isResume && !resumeOptions?.protocolText ? null : protocol.lines,
        protocol_chars: isResume && !resumeOptions?.protocolText ? null : protocolText.length,
        protocol_hash: isResume
          ? (resumeOptions?.protocolHash ?? "saved_session_protocol")
          : protocol.hash,
        provider_mode: providerMode,
        credential_storage_mode: credentialStorageMode,
        initial_agent: selectedInitialAgent,
        active_agents: runOptions?.activeAgents ?? null,
        max_session_cost_usd: runOptions?.maxSessionCostUsd ?? null,
        max_session_minutes: runOptions?.maxSessionMinutes ?? null,
        attachment_count: runOptions?.attachments.length ?? 0,
        link_count: runOptions?.links.length ?? 0,
      },
    });

    let lastLoggedMinute = 0;
    const heartbeat = window.setInterval(() => {
      const elapsedSeconds = Math.max(1, Math.floor((Date.now() - startedAt) / 1000));
      const elapsedMinutes = Math.floor(elapsedSeconds / 60);
      setOperation({
        title: isResume ? "Retomando sessao editorial" : "Sessao editorial em andamento",
        progress: 44,
        current: `Trabalho em andamento ha ${formatElapsedTime(elapsedSeconds)}.`,
        eta: `Inicio: ${startedAtLabel}`,
        status: "running",
      });
      if (elapsedMinutes > lastLoggedMinute) {
        lastLoggedMinute = elapsedMinutes;
        if (elapsedMinutes % 5 === 0) {
          appendActivity({
            level: "detail",
            title: "Sessao em andamento",
            detail: `Tempo decorrido: ${formatElapsedTime(elapsedSeconds)}. Rodadas continuam ate a aprovacao final.`,
          });
        }
        void logEvent({
          level: "info",
          category: "session.editorial.heartbeat",
          message: "editorial session heartbeat",
          context: { run_id: runId, elapsed_seconds: elapsedSeconds },
        });
      }
    }, 5000);

    try {
      const result = resumeOptions
        ? await resumeEditorialSession({
            run_id: runId,
            protocol_name: resumeOptions.protocolName ?? null,
            protocol_text: resumeOptions.protocolText ?? null,
            protocol_hash: resumeOptions.protocolHash ?? null,
            initial_agent: selectedInitialAgent,
            active_agents: runOptions?.activeAgents ?? null,
            max_session_cost_usd: runOptions?.maxSessionCostUsd ?? null,
            max_session_minutes: runOptions?.maxSessionMinutes ?? null,
            attachments:
              runOptions?.attachments && runOptions.attachments.length > 0
                ? runOptions.attachments
                : null,
            links: runOptions?.links ?? null,
          })
        : await runEditorialSession({
            run_id: runId,
            session_name: sessionName,
            prompt: promptText,
            protocol_name: protocol.name,
            protocol_text: protocolText,
            protocol_hash: protocol.hash,
            initial_agent: selectedInitialAgent,
            active_agents: runOptions?.activeAgents ?? null,
            max_session_cost_usd: runOptions?.maxSessionCostUsd ?? null,
            max_session_minutes: runOptions?.maxSessionMinutes ?? null,
            attachments: runOptions?.attachments ?? [],
            links: runOptions?.links ?? [],
          });
      window.clearInterval(heartbeat);
      setLastSessionMinutesPath(result.session_minutes_path);
      const nextAgentCards = latestAgentCards(result.agents);
      setAgentCards([
        ...nextAgentCards,
        {
          name: "Maestro",
          cli: "motor local",
          state: result.consensus_ready ? "ready" : "evidence",
          note: result.consensus_ready
            ? "unanimidade registrada"
            : "aguardando continuidade da sessao",
        },
      ]);
      setProtocolGateItems(latestProtocolGateItems(result.agents));
      setEvidenceRows([
        {
          label: "DOI",
          value: "revisado pelos agentes",
          tone: result.consensus_ready ? "ok" : "warn",
        },
        {
          label: "Links",
          value: result.consensus_ready ? "gate mecanico aprovado" : "gate nao liberado",
          tone: result.consensus_ready ? "ok" : "warn",
        },
        {
          label: "ABNT",
          value: result.consensus_ready ? "MaestroPeer READY" : "MaestroPeer nao liberado",
          tone: result.consensus_ready ? "ok" : "warn",
        },
        {
          label: "Quarentena",
          value: result.consensus_ready ? "sem blocker deterministico" : "texto bloqueado",
          tone: result.consensus_ready ? "ok" : "danger",
        },
      ]);
      setDiscussionItems((current) => [
        {
          round: "001",
          status: humanizeAgentStatus(result.status),
          note: result.consensus_ready
            ? `Texto final liberado em ${result.final_markdown_path}; ata em ${result.session_minutes_path}.`
            : `Sem unanimidade. Ata em ${result.session_minutes_path}; artefatos dos agentes em ${result.session_dir}.`,
        },
        ...current,
      ]);
      appendActivity({
        level: result.consensus_ready ? "summary" : "detail",
        title: result.consensus_ready ? "Texto final liberado" : "Sessao pausada",
        detail: `${summarizeAgentResults(result.agents)} Custo observado: ${
          result.observed_cost_usd == null
            ? "nao medido"
            : `US$ ${result.observed_cost_usd.toFixed(6)}`
        }. Log humano: ${result.human_log_path ?? "indisponivel"}.`,
      });
      setActiveAgentNow(null);

      if (result.consensus_ready) {
        setOperation({
          title: "Texto final liberado",
          progress: 100,
          current: `Unanimidade dos agentes registrada. Texto: ${result.final_markdown_path}`,
          eta: `Ata: ${result.session_minutes_path}`,
          status: "completed",
        });
        setPhaseItems([
          { label: "Protocolo", detail: "registrado", state: "done" },
          { label: "Verificacoes", detail: "concluidas", state: "done" },
          { label: "Agentes", detail: "concluidos", state: "done" },
          { label: "Entrega", detail: "unanimidade registrada", state: "done" },
        ]);
        void logEvent({
          level: "info",
          category: "session.editorial.final_available",
          message: "final editorial markdown available after real unanimous session",
          context: {
            run_id: runId,
            final_markdown_path: result.final_markdown_path,
            session_minutes_path: result.session_minutes_path,
            active_agents: result.active_agents,
            observed_cost_usd: result.observed_cost_usd,
            human_log_path: result.human_log_path,
            agents: result.agents.map((agent) => ({ name: agent.name, tone: agent.tone })),
          },
        });
      } else {
        setOperation({
          title: "Sessao pausada sem entrega final",
          progress: 66,
          current:
            result.status === "PAUSED_DRAFT_UNAVAILABLE"
              ? "Nenhum agente produziu rascunho utilizavel. A entrega segue indisponivel ate nova tentativa ou intervencao."
              : result.status === "TIME_LIMIT_REACHED"
                ? "O limite de tempo opcional foi atingido. A entrega segue indisponivel ate nova sessao ou retomada ajustada."
                : result.status === "COST_LIMIT_REACHED"
                  ? "O limite de custo opcional foi atingido antes de nova chamada paga. A entrega segue indisponivel."
                  : result.status === "PAUSED_COST_LIMIT_REQUIRED"
                    ? "Defina um limite de custo em USD para usar peers via API. Chamadas pagas nao rodam sem teto configurado pelo usuario."
                    : result.status === "PAUSED_COST_RATES_MISSING"
                      ? "Um peer via API esta selecionado, mas suas tarifas de entrada e saida ainda nao foram configuradas em Configuracoes > Agentes via API."
                      : result.status === "PAUSED_REVIEWERS_UNAVAILABLE"
                        ? "Nao ha revisor independente disponivel para o rascunho atual. Selecione pelo menos dois agentes ativos e retome a sessao."
                        : result.status === "PAUSED_REVIEWER_OPERATIONAL_OUTAGE"
                          ? "Os revisores independentes disponiveis falharam operacionalmente em rodadas consecutivas. A sessao foi pausada sem alterar o texto; ajuste CLI/API, inclua outro revisor independente ou troque o modo e retome."
                          : result.status === "PAUSED_LEGACY_RETRY_ACCOUNTING_UNKNOWN"
                            ? "Esta sessao foi criada antes do accounting persistente de retries pagos. Para nao renovar silenciosamente um orçamento já consumido, a continuacao via API foi pausada. Inicie uma nova sessao ou retome somente com peers CLI."
                            : result.status === "PAUSED_FINAL_REFERENCE_AUDIT"
                              ? "A sessao foi pausada porque o texto ainda depende de evidencia externa ou decisao do operador. Anexe/verifique a evidencia indicada na ata antes de retomar."
                              : result.status === "ALL_PEERS_FAILING"
                                ? "Todos os peers ativos retornaram erro em 3 rodadas consecutivas. Sessao pausada para nao queimar quota e tempo. Verifique conectividade, chaves de API e quotas; depois retome."
                                : "A sessao nao entregou texto final nesta chamada. Divergencias exigem novas rodadas ate unanimidade.",
          eta: `Ata: ${result.session_minutes_path}`,
          status: "paused",
        });
        setPhaseItems([
          { label: "Protocolo", detail: "registrado", state: "done" },
          { label: "Verificacoes", detail: "concluidas", state: "done" },
          { label: "Agentes", detail: "rodadas registradas", state: "done" },
          { label: "Entrega", detail: "aguardando unanimidade", state: "waiting" },
        ]);
        void logEvent({
          level: "warn",
          category: "session.editorial.blocked",
          message: "real editorial session completed without unanimous approval",
          context: {
            run_id: runId,
            status: result.status,
            session_minutes_path: result.session_minutes_path,
            session_dir: result.session_dir,
            active_agents: result.active_agents,
            observed_cost_usd: result.observed_cost_usd,
            max_session_cost_usd: result.max_session_cost_usd,
            max_session_minutes: result.max_session_minutes,
            human_log_path: result.human_log_path,
            agent_count: result.agents.length,
            latest_agents: result.agents.slice(-12).map((agent) => ({
              name: agent.name,
              role: agent.role,
              tone: agent.tone,
              status: agent.status,
              exit_code: agent.exit_code,
              output_path: agent.output_path,
            })),
            final_delivery:
              result.status === "PAUSED_REVIEWER_OPERATIONAL_OUTAGE"
                ? "paused_recoverable_reviewer_operational_outage"
                : "blocked_without_all_agent_unanimity",
          },
        });
      }
    } catch (error) {
      window.clearInterval(heartbeat);
      setActiveAgentNow(null);
      setOperation({
        title: "Sessao editorial falhou",
        progress: 42,
        current: "O Maestro nao conseguiu concluir a sessao editorial.",
        eta: "consulte diagnostico",
        status: "blocked",
      });
      setAgentCards([
        {
          name: "Claude",
          cli: "claude",
          state: "blocked",
          note: "falha antes de resultado estruturado",
        },
        {
          name: "Codex",
          cli: "codex",
          state: "blocked",
          note: "falha antes de resultado estruturado",
        },
        {
          name: "Gemini",
          cli: "agy",
          state: "blocked",
          note: "falha antes de resultado estruturado",
        },
        {
          name: "DeepSeek",
          cli: "deepseek-api",
          state: "blocked",
          note: "falha antes de resultado estruturado",
        },
        {
          name: "Grok",
          cli: "grok-api",
          state: "blocked",
          note: "falha antes de resultado estruturado",
        },
        {
          name: "Perplexity",
          cli: "perplexity-api",
          state: "blocked",
          note: "falha antes de resultado estruturado",
        },
        {
          name: "Maestro",
          cli: "motor local",
          state: "blocked",
          note: "consulte diagnostico e arquivos da sessao",
        },
      ]);
      void logEvent({
        level: "error",
        category: "session.editorial.invoke_failed",
        message: "native real editorial session invoke failed",
        context: { run_id: runId, error },
      });
    } finally {
      // Reset stop-button state regardless of how the session ended (success,
      // failure, or operator-stop). Backend STOPPED_BY_USER status arrives
      // through the same try/await branch as success/error.
      setIsStopRequested(false);
    }
  }

  // Operator-driven stop: confirm + invoke `stop_editorial_session`. The
  // backend signals the cancellation token; the in-flight CLI peer is killed
  // by `kill_process_tree` (cancel granularity 250ms via the
  // `run_resolved_command_observed` poll loop) and the in-flight API peer
  // future is dropped via `tokio::select!` in `send_with_retry_async`
  // (cancel <2s). The session loop exits with `STOPPED_BY_USER` status; the
  // existing run-completion branch handles UI cleanup.
  async function handleStopSession() {
    if (!sessionRunId) return;
    if (isStopRequested) return;
    const confirmed = window.confirm(
      'Parar a sessao atual? Drafts em andamento ficam preservados como artifacts mas sem convergencia.\n\nVoce pode retomar a sessao depois pelo botao "Continuar".',
    );
    if (!confirmed) return;
    setIsStopRequested(true);
    try {
      await stopEditorialSession(sessionRunId);
      void logEvent({
        level: "info",
        category: "session.user.stop_requested",
        message: "operator clicked stop session",
        context: { run_id: sessionRunId },
      });
    } catch (error) {
      // Reset on failed invoke so operator can retry.
      setIsStopRequested(false);
      void logEvent({
        level: "error",
        category: "session.user.stop_failed",
        message: "stop_editorial_session invoke failed",
        context: { run_id: sessionRunId, error: String(error) },
      });
    }
  }

  function updateAiCredential(provider: AiCredentialKey, value: string) {
    setAiCredentials((current) => ({ ...current, [provider]: value }));
  }

  function updateProviderInputRate(provider: ProviderRateKey, value: string) {
    setProviderInputUsdPerMillion((current) => ({ ...current, [provider]: value }));
  }

  function updateProviderOutputRate(provider: ProviderRateKey, value: string) {
    setProviderOutputUsdPerMillion((current) => ({ ...current, [provider]: value }));
  }

  function applyProviderRatesFromConfig(config: AiProviderConfig) {
    setProviderInputUsdPerMillion({
      openai:
        config.openai_input_usd_per_million == null
          ? ""
          : String(config.openai_input_usd_per_million),
      anthropic:
        config.anthropic_input_usd_per_million == null
          ? ""
          : String(config.anthropic_input_usd_per_million),
      gemini:
        config.gemini_input_usd_per_million == null
          ? ""
          : String(config.gemini_input_usd_per_million),
      deepseek:
        config.deepseek_input_usd_per_million == null
          ? ""
          : String(config.deepseek_input_usd_per_million),
      grok:
        config.grok_input_usd_per_million == null ? "" : String(config.grok_input_usd_per_million),
      perplexity:
        config.perplexity_input_usd_per_million == null
          ? ""
          : String(config.perplexity_input_usd_per_million),
    });
    setProviderOutputUsdPerMillion({
      openai:
        config.openai_output_usd_per_million == null
          ? ""
          : String(config.openai_output_usd_per_million),
      anthropic:
        config.anthropic_output_usd_per_million == null
          ? ""
          : String(config.anthropic_output_usd_per_million),
      gemini:
        config.gemini_output_usd_per_million == null
          ? ""
          : String(config.gemini_output_usd_per_million),
      deepseek:
        config.deepseek_output_usd_per_million == null
          ? ""
          : String(config.deepseek_output_usd_per_million),
      grok:
        config.grok_output_usd_per_million == null
          ? ""
          : String(config.grok_output_usd_per_million),
      perplexity:
        config.perplexity_output_usd_per_million == null
          ? ""
          : String(config.perplexity_output_usd_per_million),
    });
  }

  function chooseProviderMode(nextMode: ProviderMode) {
    setProviderMode(nextMode);
    if (nextMode === "cli") {
      // CLI mode is incompatible with API-only peers (DeepSeek, Grok and Perplexity).
      // Drop them from the peer set and reassign the initial agent so the
      // operator can never enter a state where the run silently falls back to API.
      setActiveAgents((current) => {
        const filtered = current.filter((agent) => !agentIsApiOnly(agent));
        return filtered.length === 0 ? ["claude"] : filtered;
      });
      setInitialAgent((current) => (agentIsApiOnly(current) ? "claude" : current));
    }
    void saveAiProviderConfig(nextMode);
    void logEvent({
      level: "info",
      category: "settings.provider_mode.changed",
      message: "operator changed AI provider orchestration mode",
      context: { provider_mode: nextMode },
    });
  }

  function chooseCredentialStorage(nextMode: CredentialStorageMode) {
    setCredentialStorageMode(nextMode);
    void persistBootstrapConfig(nextMode);
    void logEvent({
      level: "info",
      category: "settings.credential_storage.changed",
      message: "operator changed credential storage mode",
      context: { credential_storage_mode: nextMode },
    });
  }

  async function verifyCloudflareCredentials() {
    setIsVerifyingCloudflare(true);
    await persistBootstrapConfig();
    const accountId = cloudflareAccountId.trim() || cloudflareEnvSnapshot?.account_id || "";
    const tokenEnvVar =
      cloudflareTokenEnvVar.trim() ||
      cloudflareEnvSnapshot?.api_token_env_var ||
      "MAESTRO_CLOUDFLARE_API_TOKEN";
    setCloudflarePermissionRows([
      {
        label: "Token ativo",
        value: cloudflareTokenAvailable
          ? `verificando via ${tokenEnvVar}`
          : "ausente; informe token ou env var",
        tone: cloudflareTokenAvailable ? "pending" : "blocked",
      },
      {
        label: "Conta acessivel",
        value: accountId ? "aguardando resposta da API Cloudflare" : "account id ausente",
        tone: accountId ? "pending" : "blocked",
      },
      { label: "D1 Read/Edit", value: "aguardando resposta D1", tone: "pending" },
      { label: "Secrets Store", value: "aguardando resposta do Secrets Store", tone: "pending" },
    ]);
    void logEvent({
      level: "info",
      category: "settings.cloudflare.verify_requested",
      message: "operator requested Cloudflare credential validation",
      context: {
        account_id_present: accountId.length > 0,
        token_present: cloudflareTokenAvailable,
        token_source: cloudflareEnvSnapshot?.api_token_present
          ? "windows_env"
          : cloudflareTokenSource,
        token_env_var: tokenEnvVar,
        target_database: "bigdata_db",
        target_table: "mainsite_posts",
        persistence_database: "maestro_db",
        persistence_secret_store: "maestro",
        credential_storage_mode: credentialStorageMode,
      },
    });

    try {
      const result = await probeCloudflareCredentials({
        account_id: accountId,
        api_token: cloudflareApiToken.trim() || null,
        api_token_env_var: tokenEnvVar,
        persistence_database: "maestro_db",
        publication_database: "bigdata_db",
        secret_store: "maestro",
      });
      setCloudflarePermissionRows(result.rows);
      appendActivity({
        level: "diagnostic",
        title: "Cloudflare verificado",
        detail: result.rows.map((row) => `${row.label}: ${row.tone}`).join("; "),
      });
      void logEvent({
        level: result.rows.some((row) => row.tone === "error" || row.tone === "blocked")
          ? "warn"
          : "info",
        category: "settings.cloudflare.verify_rendered",
        message: "Cloudflare credential validation rendered in UI",
        context: {
          rows: result.rows.map((row) => ({ label: row.label, tone: row.tone })),
        },
      });
    } catch (error) {
      setCloudflarePermissionRows([
        { label: "Token ativo", value: "falha na verificacao local", tone: "error" },
        { label: "Conta acessivel", value: "nao executado", tone: "blocked" },
        { label: "D1 Read/Edit", value: "nao executado", tone: "blocked" },
        { label: "Secrets Store", value: "nao executado", tone: "blocked" },
      ]);
      void logEvent({
        level: "error",
        category: "settings.cloudflare.verify_failed",
        message: "Cloudflare credential validation failed before receiving API result",
        context: { error },
      });
    } finally {
      setIsVerifyingCloudflare(false);
    }
  }

  async function verifyAiProviderCredentials() {
    setIsVerifyingAiProviders(true);
    setAiProviderRowsState(
      aiProviderRows.map((provider) => ({
        label: provider.name,
        value: aiCredentials[provider.key].trim()
          ? "verificando credencial"
          : "API key nao informada",
        tone: aiCredentials[provider.key].trim() ? "pending" : "warn",
      })),
    );
    void logEvent({
      level: "info",
      category: "settings.ai_provider.verify_requested",
      message: "operator requested AI provider credential validation",
      context: {
        provider_mode: providerMode,
        credential_storage_mode: credentialStorageMode,
        openai_key_present: aiCredentials.openai.length > 0,
        anthropic_key_present: aiCredentials.anthropic.length > 0,
        gemini_key_present: aiCredentials.gemini.length > 0,
        deepseek_key_present: aiCredentials.deepseek.length > 0,
        grok_key_present: aiCredentials.grok.length > 0,
        perplexity_key_present: aiCredentials.perplexity.length > 0,
      },
    });

    const saved = await saveAiProviderConfig();
    if (!saved) {
      setAiProviderRowsState(
        aiProviderRows.map((provider) => ({
          label: provider.name,
          value: "verificacao nao executada: falha ao salvar",
          tone: "error",
        })),
      );
      setIsVerifyingAiProviders(false);
      return;
    }

    try {
      const result = await probeAiProviderCredentials(saved);
      setAiProviderRowsState(result.rows);
      setAiConfigStatus(`Verificado em ${formatBrazilDateTime(new Date(result.checked_at))}`);
      appendActivity({
        level: "diagnostic",
        title: "APIs verificadas",
        detail: result.rows.map((row) => `${row.label}: ${row.tone}`).join("; "),
      });
      void logEvent({
        level: result.rows.some((row) => row.tone === "error" || row.tone === "blocked")
          ? "warn"
          : "info",
        category: "settings.ai_provider.verify_completed",
        message: "AI provider credential validation completed",
        context: {
          rows: result.rows.map((row) => ({ label: row.label, tone: row.tone })),
        },
      });
    } catch (error) {
      setAiProviderRowsState(
        aiProviderRows.map((provider) => ({
          label: provider.name,
          value: "falha local na verificacao",
          tone: "error",
        })),
      );
      void logEvent({
        level: "error",
        category: "settings.ai_provider.verify_failed",
        message: "AI provider credential validation failed before receiving API result",
        context: { error },
      });
    } finally {
      setIsVerifyingAiProviders(false);
    }
  }

  async function savePostEditorDraft(
    title: string,
    author: string,
    htmlContent: string,
    isPublished: boolean,
    isAboutSite: boolean,
    confirmedAboutAction?: boolean,
    requestedPostId?: number,
  ) {
    setSessionName(title || sessionName);
    setMainSiteHtml(htmlContent);
    void logEvent({
      level: "info",
      category: "editor.posteditor.save",
      message: "operator saved PostEditor-compatible draft",
      context: {
        title,
        author,
        chars: htmlContent.length,
        is_published: isPublished,
        is_about_site: isAboutSite,
        confirmed_about_action: confirmedAboutAction ?? false,
        requested_post_id: requestedPostId ?? null,
        compatibility_target: "admin-app/MainSite/PostEditor",
      },
    });
    return true;
  }

  function openPostEditor() {
    setShowPostEditor(true);
    void logEvent({
      level: "info",
      category: "editor.posteditor.open",
      message: "operator opened PostEditor-compatible editor panel",
    });
  }

  function closePostEditor() {
    setShowPostEditor(false);
    void logEvent({
      level: "info",
      category: "editor.posteditor.close",
      message: "operator closed PostEditor-compatible editor panel",
    });
  }

  return (
    <div className="app-shell">
      <AppSidebar
        activeSection={activeSection}
        appVersion={APP_VERSION}
        credentialStorageMode={credentialStorageMode}
        onChooseSection={chooseSection}
      />

      <main className="workspace">
        <AppTopbar
          activeLabel={activeNavItem?.label ?? "Workspace"}
          sessionName={sessionName}
          isResumeLoading={isResumeLoading}
          isRunPreparing={isRunPreparing}
          isStopRequested={isStopRequested}
          runActionLabel={runActionLabel}
          sessionRunId={sessionRunId}
          onSessionNameChange={setSessionName}
          onRevalidate={() => void revalidateRuntime()}
          onRequestResume={() => void requestResumeSession()}
          onStart={startEditorialSession}
          onStop={() => void handleStopSession()}
        />

        {showResumePicker && (
          <ResumeDialog
            candidates={resumeCandidates}
            hasLoadedProtocol={hasLoadedProtocolForResume}
            protocol={protocol}
            useLoadedProtocol={useLoadedProtocolForResume}
            formatActivity={formatSessionActivity}
            onClose={() => setShowResumePicker(false)}
            onChoose={(session, useLoadedProtocol) =>
              void startResumeSession(session, useLoadedProtocol)
            }
            onUseLoadedProtocolChange={setUseLoadedProtocolForResume}
          />
        )}

        {activeSection === "session" && (
          <SessionScreen
            activeAgentLabels={activeAgentLabels}
            activeAgentNow={activeAgentNow}
            activeAgents={activeAgents}
            agentCards={agentCards}
            agentsMissingCostRates={agentsMissingCostRates}
            apiAgentsSelected={apiAgentsSelected}
            apiCostLimitRequired={apiCostLimitRequired}
            attachmentDeliveryPlans={attachmentDeliveryPlans}
            attachmentTotalBytes={attachmentTotalBytes}
            closePostEditor={closePostEditor}
            costRatesRequired={costRatesRequired}
            discussionItems={discussionItems}
            editorialPrompt={editorialPrompt}
            formalState={formalState}
            handlePromptAttachments={handlePromptAttachments}
            initialAgent={initialAgent}
            initialAgentLabel={initialAgentLabel}
            isResumeLoading={isResumeLoading}
            isRunPreparing={isRunPreparing}
            linkEvidenceState={linkEvidenceState}
            mainSiteHtml={mainSiteHtml}
            maxSessionCostUsd={maxSessionCostUsd}
            maxSessionMinutes={maxSessionMinutes}
            openPostEditor={openPostEditor}
            openSessionLedger={openSessionLedger}
            operation={operation}
            operationIndeterminate={operationIndeterminate}
            operationProgressLabel={operationProgressLabel}
            phaseItems={phaseItems}
            promptAttachments={promptAttachments}
            protocol={protocol}
            protocolGateItems={protocolGateItems}
            providerMode={providerMode}
            readyCount={readyCount}
            removePromptAttachment={removePromptAttachment}
            requestResumeSession={() => void requestResumeSession()}
            savePostEditorDraft={savePostEditorDraft}
            sessionLinks={sessionLinks}
            sessionName={sessionName}
            sessionRunId={sessionRunId}
            setEditorialPrompt={setEditorialPrompt}
            setInitialAgent={setInitialAgent}
            setMaxSessionCostUsd={setMaxSessionCostUsd}
            setMaxSessionMinutes={setMaxSessionMinutes}
            setSessionLinks={setSessionLinks}
            showPostEditor={showPostEditor}
            startEditorialSession={startEditorialSession}
            toggleActiveAgent={toggleActiveAgent}
            verbosity={verbosity}
            chooseVerbosity={chooseVerbosity}
            visibleActivity={visibleActivity}
          />
        )}

        {activeSection === "protocols" && (
          <ProtocolsScreen inputRef={inputRef} protocol={protocol} onImport={importProtocol} />
        )}

        {activeSection === "evidence" && (
          <EvidenceScreen
            evidenceRows={evidenceRows}
            linkAuditRows={linkAuditRows}
            citationAuditResult={citationAuditResult}
            isAuditing={isAuditingEvidence}
            onAudit={() => void auditEvidenceNow()}
          />
        )}

        {activeSection === "agents" && (
          <AgentsScreen
            agentCards={agentCards}
            protocolGateItems={protocolGateItems}
            sessionRunId={sessionRunId}
            onVerify={() => void verifyAgentsNow()}
          />
        )}

        {activeSection === "settings" && (
          <SettingsScreen
            activeTab={activeSettingsTab}
            onChooseTab={chooseSettingsTab}
            cloudflarePanel={
              <CloudflareSettingsPanel
                bootstrapConfigStatus={bootstrapConfigStatus}
                cloudflareAccountId={cloudflareAccountId}
                cloudflareApiToken={cloudflareApiToken}
                cloudflareEnvSnapshot={cloudflareEnvSnapshot}
                cloudflarePermissionRows={cloudflarePermissionRows}
                cloudflareTokenAvailable={cloudflareTokenAvailable}
                cloudflareTokenEnvVar={cloudflareTokenEnvVar}
                credentialStorageMode={credentialStorageMode}
                isVerifying={isVerifyingCloudflare}
                onAccountIdChange={setCloudflareAccountId}
                onApiTokenChange={setCloudflareApiToken}
                onChooseCredentialStorage={chooseCredentialStorage}
                onVerify={() => void verifyCloudflareCredentials()}
              />
            }
            providersPanel={
              <AiProviderSettingsPanel
                aiConfigStatus={aiConfigStatus}
                aiCredentials={aiCredentials}
                isSaving={isSavingAiConfig}
                isVerifying={isVerifyingAiProviders}
                probeRows={aiProviderRowsState}
                providerInputRates={providerInputUsdPerMillion}
                providerMode={providerMode}
                providerOutputRates={providerOutputUsdPerMillion}
                onChooseProviderMode={chooseProviderMode}
                onCredentialChange={updateAiCredential}
                onInputRateChange={updateProviderInputRate}
                onOutputRateChange={updateProviderOutputRate}
                onSave={() => void saveAiProviderConfig()}
                onVerify={() => void verifyAiProviderCredentials()}
              />
            }
          />
        )}

        {activeSection === "setup" && (
          <SetupScreen
            activeActionId={activeRuntimeBootstrapActionId}
            bootstrapRows={bootstrapRows}
            cloudflareEnvSnapshot={cloudflareEnvSnapshot}
            isPlanning={isPlanningRuntimeBootstrap}
            operation={operation}
            plan={runtimeBootstrapPlan}
            progressEvents={runtimeBootstrapProgress}
            result={runtimeBootstrapResult}
            sessionRunId={sessionRunId}
            onActionControl={(actionId, disposition) =>
              void controlRuntimeBootstrap(actionId, disposition)
            }
            onAuthorizeAction={(actionId) => void authorizeRuntimeBootstrapAction(actionId)}
            onOpenSettings={() => setActiveSection("settings")}
            onRefresh={() => void refreshRuntimeBootstrapPlan()}
          />
        )}
      </main>
    </div>
  );
}
