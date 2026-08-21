import {
  AlertTriangle,
  CheckCircle2,
  Clock3,
  Database,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import { credentialStorageModes, storageModeSummaries } from "../../constants";
import type {
  CloudflareEnvSnapshot,
  CloudflarePermissionRow,
  CredentialStorageMode,
} from "../../types";

type CloudflareSettingsPanelProps = {
  bootstrapConfigStatus: string;
  cloudflareAccountId: string;
  cloudflareApiToken: string;
  cloudflareEnvSnapshot: CloudflareEnvSnapshot | null;
  cloudflarePermissionRows: CloudflarePermissionRow[];
  cloudflarePublicationDatabase: string;
  cloudflarePublicationTable: string;
  cloudflareTokenAvailable: boolean;
  cloudflareTokenEnvVar: string;
  credentialStorageMode: CredentialStorageMode;
  isVerifying: boolean;
  onAccountIdChange: (value: string) => void;
  onApiTokenChange: (value: string) => void;
  onChooseCredentialStorage: (mode: CredentialStorageMode) => void;
  onPublicationDatabaseChange: (value: string) => void;
  onPublicationTableChange: (value: string) => void;
  onVerify: () => void;
};

export function CloudflareSettingsPanel({
  bootstrapConfigStatus,
  cloudflareAccountId,
  cloudflareApiToken,
  cloudflareEnvSnapshot,
  cloudflarePermissionRows,
  cloudflarePublicationDatabase,
  cloudflarePublicationTable,
  cloudflareTokenAvailable,
  cloudflareTokenEnvVar,
  credentialStorageMode,
  isVerifying,
  onAccountIdChange,
  onApiTokenChange,
  onChooseCredentialStorage,
  onPublicationDatabaseChange,
  onPublicationTableChange,
  onVerify,
}: CloudflareSettingsPanelProps) {
  return (
    <div className="panel settings-panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Ajustes</p>
          <h2>Cloudflare</h2>
        </div>
        <Database size={20} />
      </div>

      <div className="storage-mode-list" aria-label="Armazenamento de credenciais">
        {credentialStorageModes.map((item) => (
          <button
            key={item.mode}
            className={credentialStorageMode === item.mode ? "active" : ""}
            type="button"
            aria-pressed={credentialStorageMode === item.mode}
            onClick={() => onChooseCredentialStorage(item.mode)}
          >
            <strong>{item.label}</strong>
            <span>{item.detail}</span>
          </button>
        ))}
      </div>

      <div className="credential-form">
        <div className="storage-note">
          <strong>{storageModeSummaries[credentialStorageMode].title}</strong>
          <span>{storageModeSummaries[credentialStorageMode].detail}</span>
        </div>
        <div className="storage-note">
          <strong>Bootstrap local sem segredos</strong>
          <span>{bootstrapConfigStatus}</span>
        </div>
        <div className="storage-note">
          <strong>Token Cloudflare inicial</strong>
          <span>
            {cloudflareTokenAvailable
              ? `detectado via ${cloudflareEnvSnapshot?.api_token_env_var ?? cloudflareTokenEnvVar}${
                  cloudflareEnvSnapshot?.api_token_env_scope
                    ? ` (${cloudflareEnvSnapshot.api_token_env_scope})`
                    : ""
                }`
              : "nao salvo no bootstrap; informe no campo, env var ou futura cripta local"}
          </span>
        </div>
        <div className="field-group">
          <label htmlFor="cloudflare-account-id">Account ID</label>
          <input
            id="cloudflare-account-id"
            autoComplete="off"
            spellCheck={false}
            value={cloudflareAccountId}
            onChange={(event) => onAccountIdChange(event.target.value)}
            placeholder="informar no app local"
          />
        </div>
        <div className="field-group">
          <label htmlFor="cloudflare-api-token">API token</label>
          <input
            id="cloudflare-api-token"
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={cloudflareApiToken}
            onChange={(event) => onApiTokenChange(event.target.value)}
            placeholder="nunca gravar em logs ou artefatos"
          />
        </div>
        <div className="field-group">
          <label htmlFor="cloudflare-publication-database">Banco D1 de publicacao</label>
          <input
            id="cloudflare-publication-database"
            autoComplete="off"
            spellCheck={false}
            value={cloudflarePublicationDatabase}
            onChange={(event) => onPublicationDatabaseChange(event.target.value)}
            placeholder="example_db"
          />
        </div>
        <div className="field-group">
          <label htmlFor="cloudflare-publication-table">Tabela de posts</label>
          <input
            id="cloudflare-publication-table"
            autoComplete="off"
            spellCheck={false}
            value={cloudflarePublicationTable}
            onChange={(event) => onPublicationTableChange(event.target.value)}
            placeholder="mainsite_posts"
          />
        </div>
        <div className="target-grid">
          <div>
            <span>Persistencia</span>
            <strong>maestro_db</strong>
          </div>
          <div>
            <span>Secrets</span>
            <strong>Cloudflare Secrets Store</strong>
          </div>
          <div>
            <span>Publicacao</span>
            <strong>{cloudflarePublicationDatabase || "nao configurado"}</strong>
          </div>
          <div>
            <span>Tabela</span>
            <strong>{cloudflarePublicationTable || "nao configurada"}</strong>
          </div>
        </div>
        <button
          className={isVerifying ? "primary-button busy" : "primary-button"}
          type="button"
          onClick={onVerify}
          disabled={isVerifying}
        >
          {isVerifying ? <RefreshCw size={18} /> : <ShieldCheck size={18} />}
          {isVerifying ? "Verificando e preparando" : "Verificar e preparar"}
        </button>
      </div>

      <div className="status-checklist" aria-label="Permissoes Cloudflare">
        {cloudflarePermissionRows.map((item) => (
          <div className={`check-row ${item.tone}`} key={item.label}>
            {item.tone === "ok" ? (
              <CheckCircle2 size={15} />
            ) : item.tone === "blocked" || item.tone === "error" || item.tone === "warn" ? (
              <AlertTriangle size={15} />
            ) : (
              <Clock3 size={15} />
            )}
            <span>{item.label}</span>
            <strong>{item.value}</strong>
          </div>
        ))}
      </div>
    </div>
  );
}
