import {
  Activity,
  AlertTriangle,
  Bot,
  Clock3,
  Database,
  FilePlus2,
  FileText,
  Globe2,
  Link2,
  Play,
  ShieldCheck,
  Upload,
} from "lucide-react";
import type { ChangeEventHandler } from "react";
import { lazy, Suspense } from "react";
import { finalArtifacts, initialAgentOptions, verbosityOptions } from "../../constants";
import { logEvent } from "../../diagnostics";
import { attachmentDeliveryHint, formatBytes, humanizeRunStatus } from "../../helpers";
import type {
  ActivityItem,
  AgentCard,
  AttachmentDeliveryPlan,
  DiscussionRound,
  InitialAgentKey,
  OperationSnapshot,
  PhaseItem,
  PromptAttachmentPayload,
  ProtocolReadingGate,
  ProtocolSnapshot,
  ProviderMode,
  VerbosityMode,
} from "../../types";

const PostEditor = lazy(() => import("../../editor/posteditor/PostEditor"));

type ActiveAgentNow = {
  name: string;
  role: string;
  detail: string;
  state: "idle" | "running" | "finished";
};

type SessionScreenProps = {
  activeAgentLabels: string;
  activeAgentNow: ActiveAgentNow | null;
  activeAgents: InitialAgentKey[];
  agentCards: AgentCard[];
  agentsMissingCostRates: InitialAgentKey[];
  apiAgentsSelected: InitialAgentKey[];
  apiCostLimitRequired: boolean;
  attachmentDeliveryPlans: AttachmentDeliveryPlan[];
  attachmentTotalBytes: number;
  closePostEditor: () => void;
  costRatesRequired: boolean;
  discussionItems: DiscussionRound[];
  editorialPrompt: string;
  formalState: string;
  handlePromptAttachments: ChangeEventHandler<HTMLInputElement>;
  initialAgent: InitialAgentKey;
  initialAgentLabel: string;
  isResumeLoading: boolean;
  isRunPreparing: boolean;
  linkEvidenceState: string;
  mainSiteHtml: string;
  maxSessionCostUsd: string;
  maxSessionMinutes: string;
  openPostEditor: () => void;
  openSessionLedger: () => void;
  operation: OperationSnapshot;
  operationIndeterminate: boolean;
  operationProgressLabel: string;
  phaseItems: PhaseItem[];
  promptAttachments: PromptAttachmentPayload[];
  protocol: ProtocolSnapshot;
  protocolGateItems: ProtocolReadingGate[];
  providerMode: ProviderMode;
  readyCount: number;
  removePromptAttachment: (name: string, sizeBytes: number) => void;
  requestResumeSession: () => void;
  savePostEditorDraft: (
    title: string,
    author: string,
    htmlContent: string,
    isPublished: boolean,
    isAboutSite: boolean,
    confirmedAboutAction?: boolean,
    requestedPostId?: number,
  ) => Promise<boolean>;
  sessionLinks: string;
  sessionName: string;
  sessionRunId: string | null;
  setEditorialPrompt: (value: string) => void;
  setInitialAgent: (agent: InitialAgentKey) => void;
  setMaxSessionCostUsd: (value: string) => void;
  setMaxSessionMinutes: (value: string) => void;
  setSessionLinks: (value: string) => void;
  showPostEditor: boolean;
  startEditorialSession: () => void;
  toggleActiveAgent: (agent: InitialAgentKey) => void;
  verbosity: VerbosityMode;
  chooseVerbosity: (verbosity: VerbosityMode) => void;
  visibleActivity: ActivityItem[];
};

const agentIsApiOnly = (agent: InitialAgentKey) =>
  agent === "deepseek" || agent === "grok" || agent === "perplexity";

export function SessionScreen({
  activeAgentLabels,
  activeAgentNow,
  activeAgents,
  agentCards,
  agentsMissingCostRates,
  apiAgentsSelected,
  apiCostLimitRequired,
  attachmentDeliveryPlans,
  attachmentTotalBytes,
  closePostEditor,
  costRatesRequired,
  discussionItems,
  editorialPrompt,
  formalState,
  handlePromptAttachments,
  initialAgent,
  initialAgentLabel,
  isResumeLoading,
  isRunPreparing,
  linkEvidenceState,
  mainSiteHtml,
  maxSessionCostUsd,
  maxSessionMinutes,
  openPostEditor,
  openSessionLedger,
  operation,
  operationIndeterminate,
  operationProgressLabel,
  phaseItems,
  promptAttachments,
  protocol,
  protocolGateItems,
  providerMode,
  readyCount,
  removePromptAttachment,
  requestResumeSession,
  savePostEditorDraft,
  sessionLinks,
  sessionName,
  sessionRunId,
  setEditorialPrompt,
  setInitialAgent,
  setMaxSessionCostUsd,
  setMaxSessionMinutes,
  setSessionLinks,
  showPostEditor,
  startEditorialSession,
  toggleActiveAgent,
  verbosity,
  chooseVerbosity,
  visibleActivity,
}: SessionScreenProps) {
  return (
    <>
      <section className="status-grid" aria-label="Resumo">
        <div className="metric-panel">
          <ShieldCheck size={20} />
          <div>
            <span>Estado formal</span>
            <strong>{formalState}</strong>
          </div>
        </div>
        <div className="metric-panel">
          <Bot size={20} />
          <div>
            <span>Consenso</span>
            <strong>
              {readyCount}/{agentCards.length} aprovados
            </strong>
          </div>
        </div>
        <div className="metric-panel">
          <Link2 size={20} />
          <div>
            <span>Links</span>
            <strong>{linkEvidenceState}</strong>
          </div>
        </div>
        <div className="metric-panel">
          <FileText size={20} />
          <div>
            <span>Protocolo</span>
            <strong>{protocol.lines} linhas</strong>
          </div>
        </div>
      </section>

      <section className="prompt-grid" aria-label="Prompt editorial">
        <div className="panel prompt-panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Geracao</p>
              <h2>Prompt da sessao</h2>
            </div>
            <div className="panel-actions">
              <button
                className={isResumeLoading ? "secondary-button busy" : "secondary-button"}
                type="button"
                onClick={() => void requestResumeSession()}
                aria-busy={isResumeLoading}
                disabled={isRunPreparing || isResumeLoading}
              >
                <Clock3 size={18} />
                Retomar
              </button>
              <button
                className={isRunPreparing ? "primary-button busy" : "primary-button"}
                type="button"
                onClick={startEditorialSession}
                aria-busy={isRunPreparing}
                disabled={isRunPreparing}
              >
                <Play size={18} />
                {isRunPreparing ? "Preparando" : "Submeter"}
              </button>
            </div>
          </div>
          <div className="initial-agent-picker" aria-label="Agente redator inicial">
            <div>
              <span>Primeira versao</span>
              <strong>{initialAgentLabel}</strong>
            </div>
            <div className="initial-agent-buttons">
              {initialAgentOptions.map((option) => {
                const cliBlocksApiOnlyAgent = providerMode === "cli" && agentIsApiOnly(option.key);
                return (
                  <button
                    className={initialAgent === option.key ? "active" : ""}
                    type="button"
                    key={option.key}
                    onClick={() => setInitialAgent(option.key)}
                    aria-pressed={initialAgent === option.key}
                    disabled={isRunPreparing || cliBlocksApiOnlyAgent}
                    title={
                      cliBlocksApiOnlyAgent
                        ? `${option.label} so roda via API. Troque para Hibrido ou API para incluir.`
                        : option.detail
                    }
                  >
                    {option.label}
                  </button>
                );
              })}
            </div>
          </div>
          <div className="session-controls" aria-label="Controles da sessao">
            <div className="control-row">
              <div>
                <span>Peers ativos</span>
                <strong>{activeAgentLabels}</strong>
              </div>
              <div className="initial-agent-buttons">
                {initialAgentOptions.map((option) => {
                  const cliBlocksApiOnlyAgent =
                    providerMode === "cli" && agentIsApiOnly(option.key);
                  const isLastSelected =
                    activeAgents.length === 1 && activeAgents.includes(option.key);
                  return (
                    <button
                      className={activeAgents.includes(option.key) ? "active" : ""}
                      type="button"
                      key={option.key}
                      onClick={() => toggleActiveAgent(option.key)}
                      aria-pressed={activeAgents.includes(option.key)}
                      disabled={isRunPreparing || cliBlocksApiOnlyAgent || isLastSelected}
                      title={
                        cliBlocksApiOnlyAgent
                          ? `${option.label} so roda via API. Troque para Hibrido ou API para incluir.`
                          : option.detail
                      }
                    >
                      {option.label}
                    </button>
                  );
                })}
              </div>
            </div>
            {costRatesRequired && (
              <div className="session-warning" role="status">
                <AlertTriangle size={16} />
                <span>
                  Tarifas obrigatorias para API pendentes:{" "}
                  {agentsMissingCostRates
                    .map(
                      (agent) =>
                        initialAgentOptions.find((option) => option.key === agent)?.label ?? agent,
                    )
                    .join(", ")}
                  .
                </span>
              </div>
            )}
            {apiCostLimitRequired && (
              <div className="session-warning" role="status">
                <AlertTriangle size={16} />
                <span>
                  Defina um limite de custo em USD para peers via API:{" "}
                  {apiAgentsSelected
                    .map(
                      (agent) =>
                        initialAgentOptions.find((option) => option.key === agent)?.label ?? agent,
                    )
                    .join(", ")}
                  .
                </span>
              </div>
            )}
            <div className="limit-grid">
              <label title="Verificado entre rodadas e como timeout por chamada. Em branco = sem teto.">
                <Clock3 size={16} />
                <span>Tempo max. min</span>
                <input
                  value={maxSessionMinutes}
                  onChange={(event) => setMaxSessionMinutes(event.target.value)}
                  inputMode="numeric"
                  placeholder="60 (em branco = sem teto)"
                  disabled={isRunPreparing}
                />
              </label>
              <label title="Aplica-se apenas a peers em modo API. Chamadas pagas exigem teto definido pelo usuario.">
                <Database size={16} />
                <span>Custo max. USD</span>
                <input
                  value={maxSessionCostUsd}
                  onChange={(event) => setMaxSessionCostUsd(event.target.value)}
                  inputMode="decimal"
                  placeholder="5.00"
                  disabled={isRunPreparing}
                />
              </label>
            </div>
            <div className="attachments-row">
              <label className="secondary-button attachment-button">
                <Upload size={16} />
                Anexos
                <input
                  type="file"
                  multiple
                  onChange={(event) => void handlePromptAttachments(event)}
                  disabled={isRunPreparing}
                />
              </label>
              <span>
                {promptAttachments.length.toLocaleString("pt-BR")} arquivo(s),{" "}
                {formatBytes(attachmentTotalBytes)}
              </span>
            </div>
            <small className="field-hint">
              Para citações verificadas, anexe `citation-manifest.json` com schema
              `citation_manifest.v1`; o mesmo arquivo é preservado ao retomar a sessão.
            </small>
            {promptAttachments.length > 0 && (
              <div className="attachment-list">
                {attachmentDeliveryPlans.map((plan) => {
                  const hint = attachmentDeliveryHint(plan);
                  return (
                    <button
                      type="button"
                      key={`${plan.attachment.name}-${plan.attachment.size_bytes}`}
                      onClick={() =>
                        removePromptAttachment(plan.attachment.name, plan.attachment.size_bytes)
                      }
                      disabled={isRunPreparing}
                      title={`Remover anexo; previsao de entrega: ${hint}. A decisao final acontece no envio.`}
                    >
                      <span>
                        {plan.attachment.name} · {formatBytes(plan.attachment.size_bytes)}
                      </span>
                      <small>{hint}</small>
                    </button>
                  );
                })}
              </div>
            )}
            <label className="links-control">
              <span>
                <Globe2 size={16} />
                Links da sessao
              </span>
              <textarea
                value={sessionLinks}
                onChange={(event) => setSessionLinks(event.target.value)}
                placeholder="https://..."
                disabled={isRunPreparing}
              />
            </label>
          </div>
          <textarea
            className="prompt-input"
            value={editorialPrompt}
            onChange={(event) => setEditorialPrompt(event.target.value)}
            aria-label="Prompt de geracao editorial"
          />
          <div className="prompt-footer">
            <span>{editorialPrompt.length.toLocaleString("pt-BR")} caracteres</span>
            <span>entrega: unanimidade dos agentes</span>
            <span>run: {sessionRunId ?? "sem sessao"}</span>
            <span>{protocol.lines} linhas de protocolo</span>
          </div>
        </div>

        <div className="panel reading-panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Regra obrigatoria</p>
              <h2>Leitura integral</h2>
            </div>
            <ShieldCheck size={20} />
          </div>
          <div className="reading-list">
            {protocolGateItems.map((gate) => (
              <div className="reading-row" key={gate.agent}>
                <div>
                  <strong>{gate.agent}</strong>
                  <span>{gate.status}</span>
                </div>
                <div className="mini-progress" aria-label={`${gate.progress}%`}>
                  <div style={{ width: `${gate.progress}%` }} />
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section className="panel operation-panel" aria-label="Sessao editorial">
        <div className="operation-head">
          <div>
            <p className="eyebrow">Sessao</p>
            <h2>{operation.title}</h2>
            <span className={`run-state-badge ${operation.status}`}>
              {humanizeRunStatus(operation.status)}
            </span>
          </div>
          <div className="verbosity-control" aria-label="Verbosidade da interface">
            {verbosityOptions.map((option) => {
              const Icon = option.icon;
              return (
                <button
                  className={verbosity === option.mode ? "active" : ""}
                  type="button"
                  key={option.mode}
                  aria-pressed={verbosity === option.mode}
                  onClick={() => chooseVerbosity(option.mode)}
                >
                  <Icon size={16} />
                  {option.label}
                </button>
              );
            })}
          </div>
        </div>

        <div className="operation-body">
          <div className="operation-summary">
            <div className={`pulse-icon ${operation.status}`}>
              <Activity size={22} />
            </div>
            <div>
              <strong>{operation.current}</strong>
              <span>{operation.eta}</span>
            </div>
          </div>
          <div className="progress-stack" aria-label={operationProgressLabel}>
            <div className={`progress-track ${operationIndeterminate ? "indeterminate" : ""}`}>
              <div
                className={`progress-fill ${operation.status} ${operationIndeterminate ? "indeterminate" : ""}`}
                style={operationIndeterminate ? undefined : { width: `${operation.progress}%` }}
              />
            </div>
            <span>{operationProgressLabel}</span>
          </div>
          <div className={`active-agent-now ${activeAgentNow?.state ?? "idle"}`} aria-live="polite">
            <div className="agent-icon">
              <Bot size={16} />
            </div>
            <div>
              <span>Agente em turno</span>
              <strong>
                {activeAgentNow?.name ?? (isRunPreparing ? "Aguardando primeiro turno" : "Nenhum")}
              </strong>
              <em>
                {activeAgentNow?.detail ??
                  "O indicador atualiza automaticamente quando o backend inicia cada peer."}
              </em>
            </div>
          </div>
        </div>

        <div className="phase-list" aria-label="Fases da rodada">
          {phaseItems.map((phase) => (
            <div className={`phase-item ${phase.state}`} key={phase.label}>
              <div className="phase-marker" />
              <strong>{phase.label}</strong>
              <span>{phase.detail}</span>
            </div>
          ))}
        </div>

        <div className="activity-feed" aria-label="Atividade">
          {visibleActivity.map((item) => (
            <div className={`activity-row ${item.level}`} key={`${item.time}-${item.title}`}>
              <span>{item.time}</span>
              <div>
                <strong>{item.title}</strong>
                <p>{item.detail}</p>
              </div>
            </div>
          ))}
        </div>
      </section>

      <section className="panel session-ledger-panel" aria-label="Discussao editorial">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Ata viva</p>
            <h2>Discussao e entrega</h2>
          </div>
          <button
            className="secondary-button"
            type="button"
            onClick={() => void openSessionLedger()}
          >
            <FileText size={18} />
            Ver ata
          </button>
        </div>
        <div className="ledger-grid">
          <div className="round-list">
            {discussionItems.map((item) => (
              <div className="round-row" key={`${item.round}-${item.status}`}>
                <span>{item.round}</span>
                <div>
                  <strong>{item.status}</strong>
                  <p>{item.note}</p>
                </div>
              </div>
            ))}
          </div>
          <div className="artifact-list">
            {finalArtifacts.map((artifact) => (
              <div className="artifact-card" key={artifact.name}>
                <FileText size={18} />
                <div>
                  <strong>{artifact.name}</strong>
                  <span>{artifact.detail}</span>
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section className="panel posteditor-parity-panel" aria-label="Editor integrado">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Editor integrado</p>
            <h2>PostEditor parity</h2>
          </div>
          {showPostEditor ? (
            <span className="parity-badge">HTML MainSite</span>
          ) : (
            <button className="primary-button" type="button" onClick={openPostEditor}>
              <FilePlus2 size={18} />
              Criar Post
            </button>
          )}
        </div>
        {showPostEditor && (
          <Suspense
            fallback={
              <div className="posteditor-loading" role="status">
                Carregando editor...
              </div>
            }
          >
            <PostEditor
              editingPostId={null}
              initialTitle={sessionName}
              initialAuthor="Leonardo Cardozo Vargas"
              initialContent={mainSiteHtml}
              initialIsPublished={false}
              initialIsAboutSite={false}
              savingPost={false}
              showNotification={(message, type) =>
                void logEvent({
                  level: type === "error" ? "error" : "info",
                  category: "editor.posteditor.notification",
                  message,
                  context: { type },
                })
              }
              onSave={savePostEditorDraft}
              onClose={closePostEditor}
            />
          </Suspense>
        )}
      </section>
    </>
  );
}
