import { CheckCircle2, RefreshCw, Search, ShieldAlert, ShieldCheck, XCircle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import {
  listLinkIntegrityRecords,
  proposeLinkCorrections,
  reviewLinkIntegrity,
} from "../../services/evidence";
import type {
  LinkClassification,
  LinkCrossReviewStatus,
  LinkIntegrityRecord,
  LinkReviewDecision,
} from "../../types";

type LinkIntegrityPanelProps = {
  recentRecords: LinkIntegrityRecord[];
};

const classificationLabels: Record<LinkClassification, string> = {
  verified_supports_claim: "Verificado e sustenta a afirmação",
  verified_but_weak: "Verificado, mas fraco",
  redirected_verified: "Redirecionado e verificado",
  content_type_mismatch: "Tipo de conteúdo divergente",
  not_found: "Não encontrado",
  forbidden: "Acesso proibido",
  auth_required: "Autenticação necessária",
  captcha_required: "CAPTCHA necessário",
  paywall: "Conteúdo pago",
  timeout: "Tempo esgotado",
  dns_error: "Falha de DNS",
  tls_error: "Falha de TLS",
  malformed: "URL malformada",
  suspected_hallucination: "Possível alucinação",
  quarantined: "Em quarentena",
};

const crossReviewLabels: Record<LinkCrossReviewStatus, string> = {
  not_needed: "Não necessária",
  pending: "Pendente",
  accepted: "Aceita",
  rejected: "Rejeitada",
};

const reviewDecisionLabels: Record<LinkReviewDecision, string> = {
  accept: "Aceitar",
  reject: "Rejeitar",
  quarantine: "Colocar em quarentena",
};

function formatDate(value: string | null) {
  if (!value) return "não informado";
  const instant = new Date(value);
  if (Number.isNaN(instant.getTime())) return "não informado";
  return new Intl.DateTimeFormat("pt-BR", {
    timeZone: "America/Sao_Paulo",
    dateStyle: "short",
    timeStyle: "medium",
  }).format(instant);
}

function mergeRecords(current: LinkIntegrityRecord[], incoming: LinkIntegrityRecord[]) {
  const merged = new Map(current.map((record) => [record.link_id, record]));
  for (const record of incoming) merged.set(record.link_id, record);
  return [...merged.values()].sort((left, right) =>
    right.checked_at.localeCompare(left.checked_at),
  );
}

function claimSupportLabel(record: LinkIntegrityRecord) {
  if (record.claim_supported === null) return "Ainda não julgado editorialmente";
  if (record.claim_supported && record.review_decision === "accept") {
    return "Suporte à afirmação aceito explicitamente";
  }
  if (!record.claim_supported) return "Não sustenta a afirmação";
  return "Sinal armazenado sem aceite editorial concluído";
}

export function LinkIntegrityPanel({ recentRecords }: LinkIntegrityPanelProps) {
  const [records, setRecords] = useState<LinkIntegrityRecord[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [total, setTotal] = useState(0);
  const [query, setQuery] = useState("");
  const [classification, setClassification] = useState<LinkClassification | "">("");
  const [crossReviewStatus, setCrossReviewStatus] = useState<LinkCrossReviewStatus | "">("");
  const [needsReviewOnly, setNeedsReviewOnly] = useState(false);
  const [busy, setBusy] = useState<string | null>("inventory");
  const [feedback, setFeedback] = useState("Carregando o inventário de integridade.");
  const [reviewDecision, setReviewDecision] = useState<LinkReviewDecision | null>(null);
  const [reviewNote, setReviewNote] = useState("");
  const [candidateProvider, setCandidateProvider] = useState("crossref");
  const [candidateQuery, setCandidateQuery] = useState("");

  const selected = useMemo(
    () => records.find((record) => record.link_id === selectedId) ?? null,
    [records, selectedId],
  );

  useEffect(() => {
    let disposed = false;
    void listLinkIntegrityRecords({ limit: 30 })
      .then((result) => {
        if (disposed) return;
        setRecords((current) => mergeRecords(current, result.items));
        setSelectedId(result.items[0]?.link_id ?? null);
        setNextCursor(result.next_cursor);
        setTotal(result.total);
        setFeedback(
          result.total === 0
            ? "Nenhum link foi auditado ainda."
            : `${result.total.toLocaleString("pt-BR")} registros de integridade encontrados.`,
        );
      })
      .catch(() => {
        if (!disposed) {
          setFeedback("Não foi possível ler o inventário. Nenhum registro foi alterado.");
        }
      })
      .finally(() => {
        if (!disposed) setBusy(null);
      });
    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    if (recentRecords.length === 0) return;
    setRecords((current) => mergeRecords(current, recentRecords));
    setSelectedId((current) => current ?? recentRecords[0]?.link_id ?? null);
    setTotal((current) => Math.max(current, recentRecords.length));
  }, [recentRecords]);

  useEffect(() => {
    setReviewDecision(null);
    setReviewNote("");
    setCandidateQuery("");
  }, [selectedId]);

  function storeRecord(record: LinkIntegrityRecord) {
    setRecords((current) => mergeRecords(current, [record]));
    setSelectedId(record.link_id);
  }

  async function loadInventory(cursor?: string) {
    setBusy(cursor ? "more" : "inventory");
    try {
      const result = await listLinkIntegrityRecords({
        ...(query.trim() ? { query: query.trim() } : {}),
        ...(classification ? { classifications: [classification] } : {}),
        ...(crossReviewStatus ? { cross_review_statuses: [crossReviewStatus] } : {}),
        ...(needsReviewOnly ? { needs_review_only: true } : {}),
        limit: 30,
        ...(cursor ? { cursor } : {}),
      });
      setRecords((current) => (cursor ? mergeRecords(current, result.items) : result.items));
      setSelectedId((current) =>
        cursor && current ? current : (result.items[0]?.link_id ?? null),
      );
      setNextCursor(result.next_cursor);
      setTotal(result.total);
      setFeedback(`${result.total.toLocaleString("pt-BR")} registros correspondem ao filtro.`);
    } catch {
      setFeedback("A leitura do inventário falhou. Nenhuma escrita foi tentada.");
    } finally {
      setBusy(null);
    }
  }

  async function searchCandidates() {
    if (!selected) return;
    const provider = candidateProvider.trim();
    if (!provider) {
      setFeedback("Informe o provedor configurado para buscar candidatos.");
      return;
    }
    setBusy("candidates");
    try {
      const record = await proposeLinkCorrections({
        link_id: selected.link_id,
        provider,
        ...(candidateQuery.trim() ? { query: candidateQuery.trim() } : {}),
        limit: 8,
      });
      storeRecord(record);
      setFeedback(
        `${record.correction_candidates.length.toLocaleString("pt-BR")} propostas registradas. Nenhuma foi aplicada automaticamente.`,
      );
    } catch {
      setFeedback("A busca de candidatos falhou. O link e o texto permaneceram inalterados.");
    } finally {
      setBusy(null);
    }
  }

  async function submitReview() {
    if (!selected || !reviewDecision) {
      setFeedback("Escolha uma decisão explícita antes de registrar a revisão.");
      return;
    }
    const note = reviewNote.trim();
    if (!note) {
      setFeedback("Explique o julgamento editorial. A nota de revisão é obrigatória.");
      return;
    }
    setBusy("review");
    try {
      const record = await reviewLinkIntegrity({
        link_id: selected.link_id,
        decision: reviewDecision,
        note,
        reviewer: "operator",
        expected_normalized_url: selected.normalized_url,
        expected_sha256: selected.sha256,
      });
      storeRecord(record);
      setReviewDecision(null);
      setReviewNote("");
      setFeedback(
        "Decisão registrada sobre a versão verificada. Nenhuma substituição foi aplicada ao texto.",
      );
    } catch {
      setFeedback(
        "A decisão não foi registrada. Recarregue o item: a URL ou o conteúdo podem ter mudado desde a leitura.",
      );
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="panel link-integrity-panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Integridade editorial</p>
          <h2>Links auditados e revisão</h2>
        </div>
        <button
          className={busy === "inventory" ? "icon-button busy" : "icon-button"}
          type="button"
          aria-label="Atualizar inventário de links"
          disabled={busy !== null}
          onClick={() => void loadInventory()}
        >
          <RefreshCw size={18} />
        </button>
      </div>

      <p className="link-integrity-boundary">
        Resposta HTTP, redirecionamento e hash são evidências mecânicas. Mesmo HTTP 200 não prova
        que a fonte sustenta a afirmação: esse julgamento exige uma decisão editorial explícita.
      </p>

      <div className="link-integrity-layout">
        <div className="link-integrity-inventory">
          <div className="link-integrity-filters">
            <input
              className="text-input"
              type="search"
              aria-label="Buscar no inventário de links"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") void loadInventory();
              }}
              placeholder="Âncora, contexto, URL ou artefato"
            />
            <select
              aria-label="Filtrar por classificação"
              value={classification}
              onChange={(event) => setClassification(event.target.value as LinkClassification | "")}
            >
              <option value="">Todas as classificações</option>
              {Object.entries(classificationLabels).map(([value, label]) => (
                <option value={value} key={value}>
                  {label}
                </option>
              ))}
            </select>
            <select
              aria-label="Filtrar por estado de cross-review"
              value={crossReviewStatus}
              onChange={(event) =>
                setCrossReviewStatus(event.target.value as LinkCrossReviewStatus | "")
              }
            >
              <option value="">Todos os estados de revisão</option>
              {Object.entries(crossReviewLabels).map(([value, label]) => (
                <option value={value} key={value}>
                  {label}
                </option>
              ))}
            </select>
            <label className="check-row link-integrity-review-only">
              <input
                type="checkbox"
                checked={needsReviewOnly}
                onChange={(event) => setNeedsReviewOnly(event.target.checked)}
              />
              Somente pendentes de revisão
            </label>
            <button
              className="secondary-button"
              type="button"
              disabled={busy !== null}
              onClick={() => void loadInventory()}
            >
              <Search size={17} /> Aplicar filtros
            </button>
          </div>

          <p className="evidence-feedback" role="status" aria-live="polite">
            {feedback}
          </p>

          <div className="link-integrity-records" aria-label="Inventário de links auditados">
            {records.length === 0 && <p className="empty-state">Nenhum link no filtro atual.</p>}
            {records.map((record) => (
              <button
                className={`link-integrity-record ${record.tone} ${selectedId === record.link_id ? "selected" : ""}`}
                type="button"
                key={record.link_id}
                aria-pressed={selectedId === record.link_id}
                onClick={() => setSelectedId(record.link_id)}
              >
                <span>
                  <strong>{record.anchor_text || record.original_url}</strong>
                  <small>{record.surrounding_text || "Sem contexto textual preservado"}</small>
                </span>
                <span className="link-integrity-record-state">
                  <strong>{classificationLabels[record.classification]}</strong>
                  <small>Revisão: {crossReviewLabels[record.cross_review_status]}</small>
                </span>
              </button>
            ))}
          </div>

          {nextCursor && (
            <button
              className="secondary-button evidence-more-button"
              type="button"
              disabled={busy !== null}
              onClick={() => void loadInventory(nextCursor)}
            >
              Carregar mais ({records.length.toLocaleString("pt-BR")} de{" "}
              {total.toLocaleString("pt-BR")})
            </button>
          )}
        </div>

        <div className="link-integrity-detail">
          {!selected && <p className="empty-state">Selecione um registro para revisar.</p>}
          {selected && (
            <>
              <div className={`link-integrity-classification ${selected.tone}`}>
                <strong>{classificationLabels[selected.classification]}</strong>
                <span>{claimSupportLabel(selected)}</span>
              </div>

              <dl className="detail-list link-integrity-detail-list">
                <div>
                  <dt>Âncora</dt>
                  <dd>{selected.anchor_text ?? "sem texto de âncora"}</dd>
                </div>
                <div>
                  <dt>Contexto</dt>
                  <dd>{selected.surrounding_text || "não preservado"}</dd>
                </div>
                <div>
                  <dt>Original</dt>
                  <dd>{selected.original_url}</dd>
                </div>
                <div>
                  <dt>Normalizada</dt>
                  <dd>{selected.normalized_url}</dd>
                </div>
                <div>
                  <dt>Final</dt>
                  <dd>{selected.final_url ?? "sem URL final"}</dd>
                </div>
                <div>
                  <dt>Resposta</dt>
                  <dd>
                    HTTP {selected.http_status ?? "sem resposta"} ·{" "}
                    {selected.content_type ?? "tipo desconhecido"}
                  </dd>
                </div>
                <div>
                  <dt>SHA-256</dt>
                  <dd>
                    <code>{selected.sha256 ?? "não calculado"}</code>
                  </dd>
                </div>
                <div>
                  <dt>Verificada</dt>
                  <dd>{formatDate(selected.checked_at)}</dd>
                </div>
                <div>
                  <dt>Revisão</dt>
                  <dd>
                    {crossReviewLabels[selected.cross_review_status]} · decisão{" "}
                    {selected.review_decision
                      ? reviewDecisionLabels[selected.review_decision]
                      : "não registrada"}
                  </dd>
                </div>
                <div>
                  <dt>Origem</dt>
                  <dd>
                    {selected.source_artifact} · <code>{selected.source_fingerprint}</code>
                  </dd>
                </div>
                {selected.web_evidence_id && (
                  <div>
                    <dt>Evidência web</dt>
                    <dd>
                      <code>{selected.web_evidence_id}</code>
                    </dd>
                  </div>
                )}
              </dl>

              {selected.normalization_changes.length > 0 && (
                <details className="evidence-details-disclosure">
                  <summary>
                    Normalização ({selected.normalization_changes.length.toLocaleString("pt-BR")})
                  </summary>
                  <ul>
                    {selected.normalization_changes.map((change) => (
                      <li key={change}>{change}</li>
                    ))}
                  </ul>
                </details>
              )}

              {selected.redirect_chain.length > 0 && (
                <details className="evidence-details-disclosure">
                  <summary>
                    Redirecionamentos ({selected.redirect_chain.length.toLocaleString("pt-BR")})
                  </summary>
                  <ol>
                    {selected.redirect_chain.map((redirect) => (
                      <li key={`${redirect.status}-${redirect.url}`}>
                        {redirect.status} · {redirect.url}
                      </li>
                    ))}
                  </ol>
                </details>
              )}

              <section className="link-candidate-section" aria-labelledby="link-candidates-title">
                <div>
                  <p className="eyebrow">Propostas, não alterações</p>
                  <h3 id="link-candidates-title">Candidatos de correção</h3>
                </div>
                <div className="link-candidate-search">
                  <input
                    className="text-input"
                    value={candidateQuery}
                    onChange={(event) => setCandidateQuery(event.target.value)}
                    placeholder="Consulta específica (opcional)"
                    aria-label="Consulta para candidatos de correção"
                  />
                  <input
                    className="text-input"
                    value={candidateProvider}
                    onChange={(event) => setCandidateProvider(event.target.value)}
                    placeholder="crossref ou conector configurado"
                    aria-label="Provedor de candidatos"
                  />
                  <button
                    className="secondary-button"
                    type="button"
                    disabled={busy !== null}
                    onClick={() => void searchCandidates()}
                  >
                    <Search size={17} /> Buscar propostas
                  </button>
                </div>
                <p className="help-text">
                  A busca apenas registra opções. Nenhuma URL ou texto é substituído sem uma etapa
                  editorial posterior.
                </p>
                <div className="link-candidate-list">
                  {selected.correction_candidates.length === 0 && (
                    <p className="empty-state">Nenhum candidato registrado.</p>
                  )}
                  {selected.correction_candidates.map((candidate) => (
                    <article className="link-candidate" key={candidate.candidate_id}>
                      <div>
                        <strong>
                          {candidate.action === "replace"
                            ? "Substituir"
                            : candidate.action === "remove"
                              ? "Remover"
                              : "Reformular texto"}
                        </strong>
                        <span>{candidate.title || candidate.url || "Sem URL proposta"}</span>
                      </div>
                      {candidate.url && <code>{candidate.url}</code>}
                      <p>{candidate.rationale}</p>
                      <small>
                        {candidate.provider} · {formatDate(candidate.proposed_at)}
                        {candidate.query ? ` · consulta: ${candidate.query}` : ""}
                        {candidate.web_evidence_id
                          ? ` · evidência: ${candidate.web_evidence_id}`
                          : ""}
                      </small>
                    </article>
                  ))}
                </div>
              </section>

              <section className="link-review-section" aria-labelledby="link-review-title">
                <div>
                  <p className="eyebrow">Decisão humana</p>
                  <h3 id="link-review-title">Registrar julgamento</h3>
                </div>
                <fieldset className="link-review-decisions">
                  <legend>Decisão editorial</legend>
                  <button
                    className={reviewDecision === "accept" ? "selected" : ""}
                    type="button"
                    aria-pressed={reviewDecision === "accept"}
                    disabled={busy !== null}
                    onClick={() => setReviewDecision("accept")}
                  >
                    <CheckCircle2 size={17} /> Aceitar
                  </button>
                  <button
                    className={reviewDecision === "reject" ? "selected" : ""}
                    type="button"
                    aria-pressed={reviewDecision === "reject"}
                    disabled={busy !== null}
                    onClick={() => setReviewDecision("reject")}
                  >
                    <XCircle size={17} /> Rejeitar
                  </button>
                  <button
                    className={reviewDecision === "quarantine" ? "selected" : ""}
                    type="button"
                    aria-pressed={reviewDecision === "quarantine"}
                    disabled={busy !== null}
                    onClick={() => setReviewDecision("quarantine")}
                  >
                    <ShieldAlert size={17} /> Quarentena
                  </button>
                </fieldset>
                <label className="field-label" htmlFor="link-review-note">
                  Fundamentação obrigatória
                </label>
                <textarea
                  id="link-review-note"
                  className="text-area link-review-note"
                  value={reviewNote}
                  required
                  onChange={(event) => setReviewNote(event.target.value)}
                  placeholder="Explique se a fonte sustenta a afirmação e por quê. Não cole credenciais."
                />
                <button
                  className="primary-button"
                  type="button"
                  disabled={busy !== null || !reviewDecision || !reviewNote.trim()}
                  onClick={() => void submitReview()}
                >
                  <ShieldCheck size={17} /> Registrar decisão sobre esta versão
                </button>
                {selected.review_note && (
                  <div className="link-review-history">
                    <strong>Última decisão registrada</strong>
                    <p>{selected.review_note}</p>
                    <small>
                      {selected.reviewed_by ?? "operador"} · {formatDate(selected.reviewed_at)}
                    </small>
                  </div>
                )}
              </section>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
