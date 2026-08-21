import { AlertTriangle, CheckCircle2, CloudUpload, Eye } from "lucide-react";
import { useEffect, useState } from "react";
import type { MainSiteD1PublishPlan, MainSiteD1PublishResult } from "../../types";

type MainSitePublishingPanelProps = {
  busy: boolean;
  error: string | null;
  plan: MainSiteD1PublishPlan | null;
  result: MainSiteD1PublishResult | null;
  targetConfigured: boolean;
  onPreview: () => void;
  onPublish: () => void;
  onReset: () => void;
};

const actionLabel = (action: MainSiteD1PublishPlan["action"]) =>
  action === "insert" ? "criar um novo post" : "atualizar o post existente";

export function MainSitePublishingPanel({
  busy,
  error,
  plan,
  result,
  targetConfigured,
  onPreview,
  onPublish,
  onReset,
}: MainSitePublishingPanelProps) {
  const [confirmed, setConfirmed] = useState(false);

  useEffect(() => {
    setConfirmed(false);
  }, [plan?.plan_id]);

  if (result) {
    return (
      <section className="panel" aria-label="Publicacao MainSite D1">
        <div className="storage-note">
          <CheckCircle2 size={18} />
          <strong>Publicacao confirmada por readback</strong>
          <span>
            Post #{result.post_id} · versao de conteudo {result.content_version} · transporte{" "}
            {result.transport}
          </span>
        </div>
        <button type="button" onClick={onReset}>
          Preparar outra publicacao
        </button>
      </section>
    );
  }

  return (
    <section className="panel" aria-label="Publicacao MainSite D1">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Cloudflare D1</p>
          <h3>Publicacao com confirmacao e readback</h3>
        </div>
        <CloudUpload size={19} />
      </div>

      {!plan && (
        <>
          <p className="field-hint">
            O preview consulta o destino e calcula a operacao. Ele nao executa nenhuma escrita.
          </p>
          <button
            className="primary-button"
            type="button"
            onClick={onPreview}
            disabled={busy || !targetConfigured}
          >
            <Eye size={17} />
            {busy ? "Gerando preview" : "Gerar preview sem escrita"}
          </button>
        </>
      )}

      {plan && (
        <div className="credential-form">
          <div className="storage-note">
            <strong>Acao planejada</strong>
            <span>{actionLabel(plan.action)}</span>
          </div>
          <div className="storage-note">
            <strong>Intencao SQL</strong>
            <code>{plan.sql_intent}</code>
          </div>
          <div className="storage-note">
            <strong>Versao de conteudo</strong>
            <span>
              {plan.content_version_current} → {plan.content_version_next}
            </span>
          </div>
          <div className="status-checklist" aria-label="Resumo de diferencas">
            {plan.diff_summary.map((item) => (
              <div
                className={`check-row ${item.change === "unchanged" ? "ok" : "warn"}`}
                key={item.field}
              >
                {item.change === "unchanged" ? (
                  <CheckCircle2 size={15} />
                ) : (
                  <AlertTriangle size={15} />
                )}
                <span>{item.field}</span>
                <strong>{item.change}</strong>
              </div>
            ))}
          </div>
          <label className="storage-note" htmlFor="confirm-mainsite-d1-publish">
            <input
              id="confirm-mainsite-d1-publish"
              type="checkbox"
              checked={confirmed}
              onChange={(event) => setConfirmed(event.target.checked)}
            />
            <span>
              Confirmo esta operacao remota. O backend revalidara o preview antes de escrever.
            </span>
          </label>
          <div className="posteditor-actions">
            <button type="button" onClick={onReset} disabled={busy}>
              Cancelar preview
            </button>
            <button
              className="primary-button"
              type="button"
              onClick={onPublish}
              disabled={busy || !confirmed}
            >
              <CloudUpload size={17} />
              {busy ? "Publicando e relendo" : "Publicar e confirmar readback"}
            </button>
          </div>
        </div>
      )}

      {error && (
        <div className="storage-note" role="alert">
          <AlertTriangle size={17} />
          <strong>Publicacao bloqueada</strong>
          <span>{error}</span>
        </div>
      )}
    </section>
  );
}
