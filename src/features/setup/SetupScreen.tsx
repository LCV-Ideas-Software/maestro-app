import {
  Activity,
  AlertTriangle,
  CheckCircle2,
  Clock3,
  HardDriveDownload,
  Play,
  RefreshCw,
  Square,
} from "lucide-react";
import { formatBrazilDateTime, humanizeRunStatus } from "../../helpers";
import type {
  BootstrapCheckRow,
  CloudflareEnvSnapshot,
  OperationSnapshot,
  RuntimeBootstrapActionResult,
  RuntimeBootstrapDisposition,
  RuntimeBootstrapPlan,
  RuntimeBootstrapProgressEvent,
} from "../../types";

type SetupScreenProps = {
  activeActionId: string | null;
  bootstrapRows: BootstrapCheckRow[];
  cloudflareEnvSnapshot: CloudflareEnvSnapshot | null;
  isPlanning: boolean;
  operation: OperationSnapshot;
  plan: RuntimeBootstrapPlan | null;
  progressEvents: RuntimeBootstrapProgressEvent[];
  result: RuntimeBootstrapActionResult | null;
  sessionRunId: string | null;
  onActionControl: (actionId: string, disposition: RuntimeBootstrapDisposition) => void;
  onAuthorizeAction: (actionId: string) => void;
  onOpenSettings: () => void;
  onRefresh: () => void;
};

const dependencyStateLabel: Record<string, string> = {
  ready: "pronto",
  missing: "ausente",
  outdated: "desatualizado",
  misconfigured: "mal configurado",
  auth_required: "autenticacao necessaria",
  manual_action_required: "intervencao manual",
};

export function SetupScreen({
  activeActionId,
  bootstrapRows,
  cloudflareEnvSnapshot,
  isPlanning,
  operation,
  plan,
  progressEvents,
  result,
  sessionRunId,
  onActionControl,
  onAuthorizeAction,
  onOpenSettings,
  onRefresh,
}: SetupScreenProps) {
  return (
    <section className="integration-grid bootstrap-workspace" aria-label="Setup">
      <div className="panel bootstrap-inventory-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Primeira execucao</p>
            <h2>Inventario de dependencias</h2>
          </div>
          <button
            className={isPlanning ? "icon-button busy" : "icon-button"}
            type="button"
            title="Reverificar e gerar novo plano"
            onClick={onRefresh}
            disabled={isPlanning || activeActionId !== null}
            aria-busy={isPlanning}
          >
            <RefreshCw size={18} />
          </button>
        </div>
        <div className="pipeline-list">
          {bootstrapRows.map((item) => (
            <div className={`pipeline-row ${item.tone}`} key={item.label}>
              <span>{item.label}</span>
              <strong>{item.value}</strong>
            </div>
          ))}
        </div>
        {plan && (
          <div
            className={plan.required_ready ? "bootstrap-readiness ready" : "bootstrap-readiness"}
          >
            {plan.required_ready ? <CheckCircle2 size={17} /> : <AlertTriangle size={17} />}
            <span>
              {plan.required_ready
                ? "Todas as dependencias obrigatorias estao prontas."
                : "Ha dependencias obrigatorias que ainda exigem acao."}
            </span>
          </div>
        )}
      </div>

      <div className="panel bootstrap-plan-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Plano hashado</p>
            <h2>Acoes propostas</h2>
          </div>
          <HardDriveDownload size={20} />
        </div>
        {!plan && <div className="settings-status">Gerando inventario seguro…</div>}
        {plan && plan.actions.length === 0 && (
          <div className="settings-status">Nenhuma acao necessaria neste momento.</div>
        )}
        {plan && (
          <div className="bootstrap-action-list">
            {plan.actions.map((action) => {
              const dependency = plan.dependencies.find(
                (candidate) => candidate.key === action.dependency_key,
              );
              const isRunning = activeActionId === action.action_id;
              const anotherActionIsRunning = activeActionId !== null && !isRunning;
              return (
                <article className="bootstrap-action-card" key={action.action_id}>
                  <div className="bootstrap-action-heading">
                    <div>
                      <span>{dependency?.label ?? action.dependency_key}</span>
                      <strong>{action.title}</strong>
                    </div>
                    <em>{dependencyStateLabel[dependency?.state ?? ""] ?? action.kind}</em>
                  </div>
                  <p>{action.description}</p>
                  <dl className="bootstrap-action-metadata">
                    <div>
                      <dt>Fonte</dt>
                      <dd>
                        {action.source.startsWith("https://") ? (
                          <a href={action.source} target="_blank" rel="noopener noreferrer">
                            {action.source}
                          </a>
                        ) : (
                          action.source
                        )}
                      </dd>
                    </div>
                    <div>
                      <dt>Escopo</dt>
                      <dd>{action.install_scope}</dd>
                    </div>
                    <div>
                      <dt>Preview</dt>
                      <dd>
                        {action.command_preview ?? "intervencao guiada; sem comando automatico"}
                      </dd>
                    </div>
                  </dl>
                  {action.requires_elevation && (
                    <div className="session-warning">
                      <AlertTriangle size={15} />
                      <span>Exige confirmacao UAC separada somente para esta acao.</span>
                    </div>
                  )}
                  {action.requires_interaction && (
                    <div className="session-warning">
                      <Clock3 size={15} />
                      <span>
                        O fluxo pausa para login, MFA, aceite ou outra intervencao humana.
                      </span>
                    </div>
                  )}
                  <div className="bootstrap-action-buttons">
                    <button
                      className={isRunning ? "primary-button busy" : "primary-button"}
                      type="button"
                      onClick={() => onAuthorizeAction(action.action_id)}
                      disabled={isRunning || anotherActionIsRunning || isPlanning}
                      aria-busy={isRunning}
                    >
                      {isRunning ? <RefreshCw size={16} /> : <Play size={16} />}
                      {isRunning
                        ? "Executando"
                        : action.kind === "manual"
                          ? "Iniciar handoff"
                          : "Autorizar"}
                    </button>
                    {isRunning ? (
                      <button
                        className="secondary-button"
                        type="button"
                        onClick={() => onActionControl(action.action_id, "cancel")}
                      >
                        <Square size={16} />
                        Cancelar
                      </button>
                    ) : (
                      <>
                        <button
                          className="secondary-button"
                          type="button"
                          onClick={() => onActionControl(action.action_id, "retry")}
                          disabled={anotherActionIsRunning}
                        >
                          Rearmar tentativa
                        </button>
                        <button
                          className="secondary-button"
                          type="button"
                          onClick={() => onActionControl(action.action_id, "defer")}
                          disabled={anotherActionIsRunning}
                        >
                          Adiar
                        </button>
                        {!dependency?.required && (
                          <button
                            className="secondary-button"
                            type="button"
                            onClick={() => onActionControl(action.action_id, "skip")}
                            disabled={anotherActionIsRunning}
                          >
                            Ignorar opcional
                          </button>
                        )}
                        {(action.dependency_key === "deepseek_credential" ||
                          action.dependency_key === "cloudflare_credential") && (
                          <button
                            className="secondary-button"
                            type="button"
                            onClick={onOpenSettings}
                            disabled={anotherActionIsRunning}
                          >
                            Abrir ajustes seguros
                          </button>
                        )}
                      </>
                    )}
                  </div>
                </article>
              );
            })}
          </div>
        )}
      </div>

      <div className="panel bootstrap-runtime-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Runtime</p>
            <h2>Diagnostico e custodia</h2>
          </div>
          <Activity size={20} />
        </div>
        <dl className="detail-list compact">
          <div>
            <dt>Run atual</dt>
            <dd>{sessionRunId ?? "sem sessao editorial"}</dd>
          </div>
          <div>
            <dt>Estado</dt>
            <dd>{humanizeRunStatus(operation.status)}</dd>
          </div>
          <div>
            <dt>Plano</dt>
            <dd>{plan ? `${plan.plan_hash.slice(0, 12)}…` : "nao gerado"}</dd>
          </div>
          <div>
            <dt>Validade</dt>
            <dd>{plan ? formatBrazilDateTime(new Date(plan.expires_at)) : "—"}</dd>
          </div>
          <div>
            <dt>Relatorio local</dt>
            <dd>{plan?.report_path ?? "data/bootstrap/"}</dd>
          </div>
          <div>
            <dt>Cloudflare env</dt>
            <dd>
              {cloudflareEnvSnapshot?.api_token_present
                ? `token em ${cloudflareEnvSnapshot.api_token_env_var} (${cloudflareEnvSnapshot.api_token_env_scope ?? "process"})`
                : "token nao detectado"}
            </dd>
          </div>
        </dl>

        {result && (
          <div className={`bootstrap-result ${result.status}`} role="status" aria-live="polite">
            <strong>{result.message}</strong>
            <span>Bundle saneado: {result.support_bundle_path}</span>
            {(result.stdout || result.stderr) && (
              <pre>{[result.stdout, result.stderr].filter(Boolean).join("\n")}</pre>
            )}
          </div>
        )}

        <div className="bootstrap-progress" aria-label="Progresso do setup">
          {progressEvents.length === 0 ? (
            <div className="settings-status">Nenhuma acao executada nesta abertura.</div>
          ) : (
            progressEvents.map((event) => (
              <div
                className="bootstrap-progress-row"
                key={`${event.action_id}-${event.at}-${event.phase}`}
              >
                <Clock3 size={14} />
                <div>
                  <strong>{event.phase}</strong>
                  <span>{event.message}</span>
                  <small>{formatBrazilDateTime(new Date(event.at))}</small>
                </div>
              </div>
            ))
          )}
        </div>
      </div>
    </section>
  );
}
