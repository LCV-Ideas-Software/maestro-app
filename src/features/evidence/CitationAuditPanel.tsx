import { AlertTriangle, BookCheck, FileDiff, ListChecks, ShieldAlert } from "lucide-react";
import { useMemo } from "react";
import type { CitationAuditResult, MaestroPeerStatus } from "../../types";

type CitationAuditPanelProps = {
  result: CitationAuditResult | null;
  isAuditing: boolean;
};

const peerLabels: Record<MaestroPeerStatus, string> = {
  ready: "READY mecânico",
  not_ready: "NOT_READY",
  needs_evidence: "NEEDS_EVIDENCE",
};

function formatAuditDate(value: string) {
  const instant = new Date(value);
  if (Number.isNaN(instant.getTime())) return "horário indisponível";
  return new Intl.DateTimeFormat("pt-BR", {
    timeZone: "America/Sao_Paulo",
    dateStyle: "short",
    timeStyle: "medium",
  }).format(instant);
}

export function CitationAuditPanel({ result, isAuditing }: CitationAuditPanelProps) {
  const needsEvidenceCount = useMemo(
    () => result?.blockers.filter((blocker) => blocker.needs_evidence).length ?? 0,
    [result],
  );
  const quarantinedCount = useMemo(
    () =>
      result?.citations.filter((citation) => citation.verification_status === "quarantined")
        .length ?? 0,
    [result],
  );

  return (
    <section className="panel citation-audit-panel" aria-labelledby="citation-audit-heading">
      <div className="panel-heading citation-audit-heading">
        <div>
          <p className="eyebrow">ABNT Citation Engine</p>
          <h2 id="citation-audit-heading">Auditoria mecânica de citações</h2>
        </div>
        {result ? (
          <span className={`citation-peer-status ${result.maestro_peer_status}`}>
            <BookCheck size={17} />
            MaestroPeer: {peerLabels[result.maestro_peer_status]}
          </span>
        ) : (
          <span
            className={isAuditing ? "citation-peer-status running" : "citation-peer-status idle"}
          >
            <ListChecks size={17} />
            {isAuditing ? "Auditando" : "Aguardando auditoria"}
          </span>
        )}
      </div>

      <div className="citation-trust-boundary">
        <ShieldAlert size={19} />
        <div>
          <strong>O veredito é determinístico, não uma aprovação por consenso de IA.</strong>
          <span>
            `READY mecânico` informa somente que os gates desta auditoria não encontraram blocker.
            Evidência, protocolo e demais gates editoriais continuam obrigatórios.
          </span>
        </div>
      </div>

      {!result && !isAuditing && (
        <p className="empty-state">
          Use “Auditar links e citações” para inspecionar o texto e o `citation_manifest.v1` anexado
          à sessão, quando houver, e calcular o status real do MaestroPeer.
        </p>
      )}
      {!result && isAuditing && (
        <p className="citation-audit-progress" role="status" aria-live="polite">
          Conferindo citações, localizadores, pares citação–referência e evidência bibliográfica.
        </p>
      )}

      {result && (
        <>
          <div className="citation-audit-meta">
            <span>
              Auditoria <code>{result.audit_id}</code>
            </span>
            <span>{formatAuditDate(result.checked_at)}</span>
            <span>
              Protocolo <code>{result.protocol_hash ?? "não fixado"}</code>
            </span>
          </div>

          <div className="citation-audit-metrics" aria-label="Resumo da auditoria de citações">
            <div>
              <span>Citações</span>
              <strong>{result.citations.length.toLocaleString("pt-BR")}</strong>
            </div>
            <div>
              <span>Referências normalizadas</span>
              <strong>{result.normalized_references.length.toLocaleString("pt-BR")}</strong>
            </div>
            <div className={result.blockers.length > 0 ? "danger" : "ok"}>
              <span>Blockers</span>
              <strong>{result.blockers.length.toLocaleString("pt-BR")}</strong>
            </div>
            <div className={needsEvidenceCount > 0 ? "warn" : "ok"}>
              <span>Needs evidence</span>
              <strong>{needsEvidenceCount.toLocaleString("pt-BR")}</strong>
            </div>
            <div className={quarantinedCount > 0 ? "danger" : "ok"}>
              <span>Quarentena</span>
              <strong>{quarantinedCount.toLocaleString("pt-BR")}</strong>
            </div>
          </div>

          <div className="citation-audit-layout">
            <section className="citation-audit-column" aria-labelledby="citation-list-heading">
              <div className="citation-section-heading">
                <div>
                  <p className="eyebrow">Manifesto estruturado</p>
                  <h3 id="citation-list-heading">Citações detectadas</h3>
                </div>
                <ListChecks size={18} />
              </div>
              <div className="citation-record-list">
                {result.citations.length === 0 && (
                  <p className="empty-state">Nenhuma citação estruturada foi detectada.</p>
                )}
                {result.citations.map((citation) => (
                  <article
                    className={`citation-record ${citation.verification_status}`}
                    key={`${citation.claim_id}-${citation.source_id}-${citation.citation_type}`}
                  >
                    <div>
                      <strong>
                        {citation.author_display ||
                          citation.author_key ||
                          "Autoria não identificada"}
                      </strong>
                      <span>
                        {citation.citation_type} · {citation.year || "ano ausente"} ·{" "}
                        {citation.locator || "sem localizador"}
                      </span>
                    </div>
                    <small>{citation.verification_status}</small>
                    <dl>
                      <div>
                        <dt>Claim</dt>
                        <dd>{citation.claim_id}</dd>
                      </div>
                      <div>
                        <dt>Fonte</dt>
                        <dd>{citation.source_id}</dd>
                      </div>
                      <div>
                        <dt>Acesso</dt>
                        <dd>{citation.source_access}</dd>
                      </div>
                      <div>
                        <dt>Risco</dt>
                        <dd>{citation.risk_if_wrong}</dd>
                      </div>
                    </dl>
                    {(citation.normalized_text || citation.original_text) && (
                      <p>{citation.normalized_text || citation.original_text}</p>
                    )}
                    {citation.normalized_footnote && (
                      <p>Nota normalizada: {citation.normalized_footnote}</p>
                    )}
                  </article>
                ))}
              </div>
            </section>

            <section className="citation-audit-column" aria-labelledby="citation-blockers-heading">
              <div className="citation-section-heading">
                <div>
                  <p className="eyebrow">Gate de publicação</p>
                  <h3 id="citation-blockers-heading">Blockers mecânicos</h3>
                </div>
                <AlertTriangle size={18} />
              </div>
              <div className="citation-blocker-list">
                {result.blockers.length === 0 && (
                  <p className="empty-state">
                    Nenhum blocker nesta auditoria. Isso não substitui os demais gates editoriais.
                  </p>
                )}
                {result.blockers.map((blocker) => (
                  <article
                    className={`citation-blocker ${blocker.needs_evidence ? "needs-evidence" : "blocking"}`}
                    key={`${blocker.code}-${blocker.claim_id ?? "none"}-${blocker.source_id ?? "none"}-${blocker.message}`}
                  >
                    <div className="citation-blocker-head">
                      <strong>{blocker.code}</strong>
                      <span>{blocker.severity}</span>
                    </div>
                    <p>{blocker.message}</p>
                    {(blocker.claim_id || blocker.source_id) && (
                      <small>
                        {blocker.claim_id ? `Claim ${blocker.claim_id}` : ""}
                        {blocker.claim_id && blocker.source_id ? " · " : ""}
                        {blocker.source_id ? `Fonte ${blocker.source_id}` : ""}
                      </small>
                    )}
                    {blocker.excerpt && <blockquote>{blocker.excerpt}</blockquote>}
                    {blocker.needs_evidence && <em>NEEDS_EVIDENCE</em>}
                  </article>
                ))}
              </div>
            </section>
          </div>

          <div className="citation-reference-grid">
            <details open>
              <summary>
                Referências normalizadas (
                {result.normalized_references.length.toLocaleString("pt-BR")})
              </summary>
              {result.normalized_references.length === 0 ? (
                <p>Nenhuma referência normalizada.</p>
              ) : (
                <ol>
                  {[...new Set(result.normalized_references)].map((reference) => (
                    <li key={reference}>{reference}</li>
                  ))}
                </ol>
              )}
            </details>
            <details>
              <summary>
                Markdown ({result.markdown_references.length.toLocaleString("pt-BR")})
              </summary>
              <pre>{result.markdown_references.join("\n\n") || "Nenhuma saída Markdown."}</pre>
            </details>
            <details>
              <summary>HTML ({result.html_references.length.toLocaleString("pt-BR")})</summary>
              <pre>{result.html_references.join("\n") || "Nenhuma saída HTML."}</pre>
            </details>
          </div>

          <div className="citation-output-grid">
            <section>
              <div className="citation-section-heading">
                <div>
                  <p className="eyebrow">Rastreabilidade</p>
                  <h3>Tabela de auditoria</h3>
                </div>
                <ListChecks size={18} />
              </div>
              <pre>{result.audit_table_markdown || "Tabela não gerada."}</pre>
            </section>
            <section>
              <div className="citation-section-heading">
                <div>
                  <p className="eyebrow">Mudanças semânticas</p>
                  <h3>Diff de citações</h3>
                </div>
                <FileDiff size={18} />
              </div>
              <pre>{result.semantic_diff || "Nenhuma mudança semântica registrada."}</pre>
            </section>
          </div>
        </>
      )}
    </section>
  );
}
