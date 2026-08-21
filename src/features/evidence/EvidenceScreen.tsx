import { Database, Globe2, Link2, RefreshCw } from "lucide-react";
import { webEvidenceTools } from "../../constants";
import type { EvidenceRow, LinkAuditResult } from "../../types";

type EvidenceScreenProps = {
  evidenceRows: EvidenceRow[];
  invalidLinkRows: LinkAuditResult["rows"];
  isAuditing: boolean;
  onAudit: () => void;
};

export function EvidenceScreen({
  evidenceRows,
  invalidLinkRows,
  isAuditing,
  onAudit,
}: EvidenceScreenProps) {
  return (
    <section className="main-grid" aria-label="Evidencias">
      <div className="panel evidence-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Motor mecanico</p>
            <h2>Evidencias</h2>
          </div>
          <button
            className={isAuditing ? "secondary-button busy" : "secondary-button"}
            type="button"
            onClick={onAudit}
            disabled={isAuditing}
            aria-busy={isAuditing}
          >
            {isAuditing ? <RefreshCw size={18} /> : <Link2 size={18} />}
            {isAuditing ? "Auditando" : "Auditar links"}
          </button>
        </div>

        <div className="evidence-grid">
          {evidenceRows.map((item) => (
            <div className={`evidence-tile ${item.tone}`} key={item.label}>
              <span>{item.label}</span>
              <strong>{item.value}</strong>
            </div>
          ))}
        </div>
        {invalidLinkRows.length > 0 && (
          <div className="link-audit-list" aria-label="Links com problema">
            {invalidLinkRows.map((row) => (
              <div className={`link-audit-row ${row.tone}`} key={`${row.url}-${row.status}`}>
                <div>
                  <strong>{row.url}</strong>
                  <span>{row.invalidity || row.status}</span>
                </div>
                <small>{row.status}</small>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Web evidence</p>
            <h2>Coleta assistida</h2>
          </div>
          <Globe2 size={20} />
        </div>
        <div className="pipeline-list">
          {webEvidenceTools.map((tool) => (
            <div className="pipeline-row" key={tool.label}>
              <span>{tool.label}</span>
              <strong>{tool.value}</strong>
            </div>
          ))}
        </div>
      </div>

      <div className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Cloudflare D1</p>
            <h2>mainsite_posts</h2>
          </div>
          <Database size={20} />
        </div>
        <dl className="detail-list compact">
          <div>
            <dt>Banco</dt>
            <dd>bigdata_db</dd>
          </div>
          <div>
            <dt>Campos</dt>
            <dd>id, title, content, author, is_published</dd>
          </div>
          <div>
            <dt>Regra</dt>
            <dd>API principal; wrangler@latest fallback</dd>
          </div>
        </dl>
      </div>
    </section>
  );
}
