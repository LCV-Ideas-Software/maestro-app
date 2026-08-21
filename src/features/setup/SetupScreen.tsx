import { Activity, HardDriveDownload } from "lucide-react";
import { humanizeRunStatus } from "../../helpers";
import type { BootstrapCheckRow, CloudflareEnvSnapshot, OperationSnapshot } from "../../types";

type SetupScreenProps = {
  bootstrapRows: BootstrapCheckRow[];
  cloudflareEnvSnapshot: CloudflareEnvSnapshot | null;
  operation: OperationSnapshot;
  sessionRunId: string | null;
};

export function SetupScreen({
  bootstrapRows,
  cloudflareEnvSnapshot,
  operation,
  sessionRunId,
}: SetupScreenProps) {
  return (
    <section className="integration-grid" aria-label="Setup">
      <div className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Primeira execucao</p>
            <h2>Bootstrap</h2>
          </div>
          <HardDriveDownload size={20} />
        </div>
        <div className="pipeline-list">
          {bootstrapRows.map((item) => (
            <div className={`pipeline-row ${item.tone}`} key={item.label}>
              <span>{item.label}</span>
              <strong>{item.value}</strong>
            </div>
          ))}
        </div>
      </div>

      <div className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Runtime</p>
            <h2>Diagnostico</h2>
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
            <dt>Logs</dt>
            <dd>um arquivo de diagnostico por execucao do app</dd>
          </div>
          <div>
            <dt>Config inicial</dt>
            <dd>data/config/bootstrap.json sem segredos</dd>
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
      </div>
    </section>
  );
}
