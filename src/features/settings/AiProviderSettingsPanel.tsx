import { AlertTriangle, CheckCircle2, Clock3, KeyRound, ListChecks } from "lucide-react";
import { aiProviderRows, providerRateRows } from "../../constants";
import type {
  AiCredentialKey,
  AiProviderProbeRow,
  ProviderMode,
  ProviderRateKey,
} from "../../types";

type AiProviderSettingsPanelProps = {
  aiConfigStatus: string;
  aiCredentials: Record<AiCredentialKey, string>;
  isSaving: boolean;
  isVerifying: boolean;
  probeRows: AiProviderProbeRow[];
  providerInputRates: Record<ProviderRateKey, string>;
  providerMode: ProviderMode;
  providerOutputRates: Record<ProviderRateKey, string>;
  onChooseProviderMode: (mode: ProviderMode) => void;
  onCredentialChange: (provider: AiCredentialKey, value: string) => void;
  onInputRateChange: (provider: ProviderRateKey, value: string) => void;
  onOutputRateChange: (provider: ProviderRateKey, value: string) => void;
  onSave: () => void;
  onVerify: () => void;
};

export function AiProviderSettingsPanel({
  aiConfigStatus,
  aiCredentials,
  isSaving,
  isVerifying,
  probeRows,
  providerInputRates,
  providerMode,
  providerOutputRates,
  onChooseProviderMode,
  onCredentialChange,
  onInputRateChange,
  onOutputRateChange,
  onSave,
  onVerify,
}: AiProviderSettingsPanelProps) {
  return (
    <div className="panel settings-panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Ajustes</p>
          <h2>Agentes via API</h2>
        </div>
        <KeyRound size={20} />
      </div>

      <div className="provider-mode" aria-label="Modo dos provedores">
        {(["hybrid", "cli", "api"] as const).map((mode) => (
          <button
            key={mode}
            className={providerMode === mode ? "active" : ""}
            type="button"
            aria-pressed={providerMode === mode}
            onClick={() => onChooseProviderMode(mode)}
          >
            {mode === "hybrid" ? "Hibrido" : mode.toUpperCase()}
          </button>
        ))}
      </div>
      <div className="provider-mode-note">
        <strong>Execucao API real por peer</strong>
        <span>
          <strong>API</strong> roda os 6 peers via provedores oficiais. <strong>Hibrido</strong>{" "}
          reserva DeepSeek, Grok e Perplexity para API (nao tem CLI) e Claude, Codex, Gemini via
          Antigravity CLI (agy), sempre, independentemente das chaves. <strong>CLI</strong> roda os
          3 peers com CLI; DeepSeek, Grok e Perplexity ficam desabilitados porque nao possuem
          integracao CLI. Tarifas continuam obrigatorias para qualquer chamada de API.
        </span>
      </div>

      <div className="ai-credential-list">
        {aiProviderRows.map((provider) => (
          <div className="credential-row" key={provider.key}>
            <div>
              <strong>{provider.name}</strong>
              <span>CLI: {provider.cli}</span>
            </div>
            <label>
              {provider.secretLabel}
              <input
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={aiCredentials[provider.key]}
                onChange={(event) => onCredentialChange(provider.key, event.target.value)}
                placeholder="informar no app local"
              />
            </label>
            <em>{provider.meta}</em>
          </div>
        ))}
      </div>

      <div className="rate-card-panel" aria-label="Tabela de tarifas dos provedores">
        <div>
          <strong>Tabela de tarifas</strong>
          <span>
            Valores em USD por 1M tokens. O limite de custo continua sendo unico por sessao; esta
            tabela apenas calcula e audita consumo observado. Sem fallback por env var.
          </span>
        </div>
        <div className="rate-card-table">
          <div className="rate-card-head" aria-hidden="true">
            <span>Provedor</span>
            <span>Entrada</span>
            <span>Saida</span>
          </div>
          {providerRateRows.map((provider) => (
            <div className="rate-card-row" key={provider.key}>
              <div>
                <strong>{provider.name}</strong>
                <span>{provider.hint}</span>
              </div>
              <label>
                <span>Entrada USD / 1M</span>
                <input
                  inputMode="decimal"
                  value={providerInputRates[provider.key]}
                  onChange={(event) => onInputRateChange(provider.key, event.target.value)}
                  placeholder="ex.: 0.55"
                />
              </label>
              <label>
                <span>Saida USD / 1M</span>
                <input
                  inputMode="decimal"
                  value={providerOutputRates[provider.key]}
                  onChange={(event) => onOutputRateChange(provider.key, event.target.value)}
                  placeholder="ex.: 2.19"
                />
              </label>
            </div>
          ))}
        </div>
      </div>

      <div className="settings-status" role="status" aria-live="polite">
        {aiConfigStatus}
      </div>

      <div className="button-row">
        <button
          className={isSaving ? "secondary-button busy" : "secondary-button"}
          type="button"
          onClick={onSave}
          disabled={isSaving || isVerifying}
          aria-busy={isSaving}
        >
          <KeyRound size={18} />
          {isSaving ? "Salvando" : "Salvar APIs"}
        </button>
        <button
          className={isVerifying ? "secondary-button busy" : "secondary-button"}
          type="button"
          onClick={onVerify}
          disabled={isSaving || isVerifying}
          aria-busy={isVerifying}
        >
          <ListChecks size={18} />
          {isVerifying ? "Verificando" : "Verificar APIs"}
        </button>
      </div>

      <div className="check-list compact-checks" aria-label="Resultado da verificacao das APIs">
        {probeRows.map((item) => (
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
