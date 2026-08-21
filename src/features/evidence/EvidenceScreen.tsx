import {
  Database,
  ExternalLink,
  FileUp,
  Globe2,
  Link2,
  MonitorUp,
  Play,
  RefreshCw,
  RotateCcw,
  Search,
  ShieldCheck,
} from "lucide-react";
import type { ChangeEvent } from "react";
import { useEffect, useMemo, useState } from "react";
import {
  fetchWebEvidence,
  getWebEvidence,
  importOperatorEvidence,
  listWebEvidence,
  openWebEvidenceInDefaultBrowser,
  replayWebEvidence,
  resumeWebEvidenceInteraction,
  searchWebEvidence,
  startRenderedWebEvidence,
} from "../../services/evidence";
import { listenToWebEvidenceProgress } from "../../services/nativeEvents";
import type {
  CitationAuditResult,
  EvidenceRow,
  LinkAuditResult,
  WebEvidenceMethod,
  WebEvidenceProgressEvent,
  WebEvidenceRecord,
  WebEvidenceState,
} from "../../types";
import { CitationAuditPanel } from "./CitationAuditPanel";
import { LinkIntegrityPanel } from "./LinkIntegrityPanel";

type EvidenceScreenProps = {
  evidenceRows: EvidenceRow[];
  linkAuditRows: LinkAuditResult["rows"];
  citationAuditResult: CitationAuditResult | null;
  isAuditing: boolean;
  onAudit: () => void;
};

const MAX_CAPTURE_BYTES = 16 * 1024 * 1024;
const acceptedCaptureExtensions = [
  ".html",
  ".htm",
  ".md",
  ".markdown",
  ".pdf",
  ".png",
  ".jpg",
  ".jpeg",
  ".webp",
  ".txt",
];
const acceptedCaptureMimeTypes = new Set([
  "text/html",
  "text/markdown",
  "text/plain",
  "application/pdf",
  "image/png",
  "image/jpeg",
  "image/webp",
]);
const captureMimeByExtension: Record<string, string> = {
  ".html": "text/html",
  ".htm": "text/html",
  ".md": "text/markdown",
  ".markdown": "text/markdown",
  ".pdf": "application/pdf",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".txt": "text/plain",
};

const stateLabels: Record<WebEvidenceState, string> = {
  queued: "Na fila",
  collecting: "Coletando",
  ready: "Pronta",
  stale: "Desatualizada",
  operator_action_required: "Ação humana",
  blocked: "Bloqueada",
  failed: "Falhou",
};

function formatEvidenceDate(value: string | null) {
  if (!value) return "não informado";
  const instant = new Date(value);
  if (Number.isNaN(instant.getTime())) return "não informado";
  return new Intl.DateTimeFormat("pt-BR", {
    timeZone: "America/Sao_Paulo",
    dateStyle: "short",
    timeStyle: "medium",
  }).format(instant);
}

function formatBytes(value: number | null) {
  if (value === null) return "não informado";
  const divisor = value >= 1_048_576 ? 1_048_576 : value >= 1_024 ? 1_024 : 1;
  const unit = value >= 1_048_576 ? "megabyte" : value >= 1_024 ? "kilobyte" : "byte";
  return new Intl.NumberFormat("pt-BR", {
    style: "unit",
    unit,
    unitDisplay: "short",
    maximumFractionDigits: 1,
  }).format(value / divisor);
}

function fileToBase64(file: File) {
  return file.arrayBuffer().then((buffer) => {
    const bytes = new Uint8Array(buffer);
    let binary = "";
    for (let offset = 0; offset < bytes.length; offset += 0x8000) {
      binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
    }
    return btoa(binary);
  });
}

function captureMediaType(file: File) {
  const browserType = file.type.toLocaleLowerCase("pt-BR");
  if (browserType) return acceptedCaptureMimeTypes.has(browserType) ? browserType : null;
  const lowerName = file.name.toLocaleLowerCase("pt-BR");
  const extension = acceptedCaptureExtensions.find((candidate) => lowerName.endsWith(candidate));
  return extension ? captureMimeByExtension[extension] : null;
}

function isAcceptedCapture(file: File) {
  return captureMediaType(file) !== null;
}

function mergeEvidence(current: WebEvidenceRecord[], incoming: WebEvidenceRecord[]) {
  const merged = new Map(current.map((record) => [record.id, record]));
  for (const record of incoming) merged.set(record.id, record);
  return [...merged.values()].sort((left, right) =>
    right.updated_at.localeCompare(left.updated_at),
  );
}

export function EvidenceScreen({
  evidenceRows,
  linkAuditRows,
  citationAuditResult,
  isAuditing,
  onAudit,
}: EvidenceScreenProps) {
  const invalidLinkRows = useMemo(
    () =>
      linkAuditRows.filter(
        (row) => row.tone === "error" || row.tone === "blocked" || row.tone === "warn",
      ),
    [linkAuditRows],
  );
  const [targetUrl, setTargetUrl] = useState("");
  const [method, setMethod] = useState<WebEvidenceMethod>("GET");
  const [forceRevalidate, setForceRevalidate] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchProvider, setSearchProvider] = useState("crossref");
  const [filterQuery, setFilterQuery] = useState("");
  const [records, setRecords] = useState<WebEvidenceRecord[]>([]);
  const [selectedEvidenceId, setSelectedEvidenceId] = useState<string | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [totalRecords, setTotalRecords] = useState(0);
  const [busyAction, setBusyAction] = useState<string | null>("inventory");
  const [feedback, setFeedback] = useState("Carregando o cache local de evidências.");
  const [progressEvents, setProgressEvents] = useState<WebEvidenceProgressEvent[]>([]);
  const [captureUrl, setCaptureUrl] = useState("");
  const [captureNotes, setCaptureNotes] = useState("");
  const [captureFile, setCaptureFile] = useState<File | null>(null);

  const selectedEvidence = useMemo(
    () => records.find((record) => record.id === selectedEvidenceId) ?? null,
    [records, selectedEvidenceId],
  );
  const queueSummary = useMemo(
    () => ({
      http:
        records.filter(
          (record) =>
            record.access_mode === "http_fetch" &&
            (record.state === "queued" || record.state === "collecting"),
        ).length + (busyAction === "fetch" || busyAction === "replay" ? 1 : 0),
      search:
        records.filter(
          (record) =>
            record.access_mode === "official_api" &&
            (record.state === "queued" || record.state === "collecting"),
        ).length + (busyAction === "search" ? 1 : 0),
      rendered:
        records.filter(
          (record) =>
            record.access_mode === "rendered_fetch" &&
            (record.state === "queued" ||
              record.state === "collecting" ||
              record.state === "operator_action_required"),
        ).length + (busyAction === "render" ? 1 : 0),
      ready: records.filter((record) => record.state === "ready").length,
      stale: records.filter((record) => record.state === "stale").length,
      interactive: records.filter((record) => record.state === "operator_action_required").length,
    }),
    [busyAction, records],
  );

  useEffect(() => {
    let disposed = false;
    void listWebEvidence({ limit: 50 })
      .then((result) => {
        if (disposed) return;
        setRecords(result.items);
        setNextCursor(result.next_cursor);
        setTotalRecords(result.total);
        setSelectedEvidenceId(result.items[0]?.id ?? null);
        setFeedback(
          result.total === 0
            ? "O cache está vazio. Faça uma coleta ou busca para começar."
            : `${result.total.toLocaleString("pt-BR")} evidências encontradas no cache.`,
        );
      })
      .catch(() => {
        if (!disposed) setFeedback("Não foi possível ler o cache. Nenhuma evidência foi alterada.");
      })
      .finally(() => {
        if (!disposed) setBusyAction(null);
      });

    let unlisten: (() => void) | null = null;
    void listenToWebEvidenceProgress((event) => {
      if (disposed) return;
      setProgressEvents((current) => [event, ...current].slice(0, 30));
      setFeedback(event.message);
    }).then((registered) => {
      if (disposed) registered();
      else unlisten = registered;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  function storeRecord(record: WebEvidenceRecord) {
    const alreadyKnown = records.some((item) => item.id === record.id);
    setRecords((current) => mergeEvidence(current, [record]));
    setSelectedEvidenceId(record.id);
    if (!alreadyKnown) setTotalRecords((current) => current + 1);
  }

  async function refreshInventory(cursor: string | null = null) {
    setBusyAction(cursor ? "more" : "inventory");
    try {
      const result = await listWebEvidence({
        query: filterQuery.trim() || null,
        limit: 50,
        cursor,
      });
      setRecords((current) => (cursor ? mergeEvidence(current, result.items) : result.items));
      setNextCursor(result.next_cursor);
      setTotalRecords(result.total);
      if (!cursor) setSelectedEvidenceId(result.items[0]?.id ?? null);
      setFeedback(`${result.total.toLocaleString("pt-BR")} evidências correspondem ao filtro.`);
    } catch {
      setFeedback("A leitura do cache falhou. Nenhuma escrita foi tentada.");
    } finally {
      setBusyAction(null);
    }
  }

  function normalizedPublicUrl() {
    try {
      const parsed = new URL(targetUrl.trim());
      return parsed.protocol === "http:" || parsed.protocol === "https:" ? parsed.href : null;
    } catch {
      return null;
    }
  }

  async function collectHttpEvidence() {
    const url = normalizedPublicUrl();
    if (!url) {
      setFeedback("Informe uma URL pública http ou https válida.");
      return;
    }
    setBusyAction("fetch");
    setFeedback("Coleta HTTP iniciada; políticas de rede e robots serão verificadas.");
    try {
      const record = await fetchWebEvidence({ url, method, force_revalidate: forceRevalidate });
      storeRecord(record);
      setFeedback(`Coleta concluída: ${stateLabels[record.state]}.`);
    } catch {
      setFeedback("A coleta falhou de forma fechada. Consulte o estado persistido no inventário.");
      await refreshInventory();
    } finally {
      setBusyAction(null);
    }
  }

  async function collectRenderedEvidence() {
    const url = normalizedPublicUrl();
    if (!url) {
      setFeedback("Informe uma URL pública http ou https válida.");
      return;
    }
    setBusyAction("render");
    try {
      const record = await startRenderedWebEvidence(url);
      storeRecord(record);
      setFeedback(
        "Handoff de renderização registrado. A coleta permanece pausada até a etapa humana e a importação do artefato.",
      );
    } catch {
      setFeedback(
        "Não foi possível iniciar a coleta renderizada. Nenhum perfil do navegador foi lido.",
      );
    } finally {
      setBusyAction(null);
    }
  }

  async function openInDefaultBrowser() {
    const url = normalizedPublicUrl();
    if (!url) {
      setFeedback("Informe uma URL pública http ou https válida.");
      return;
    }
    setBusyAction("browser");
    try {
      const record = await openWebEvidenceInDefaultBrowser(url);
      storeRecord(record);
      setCaptureUrl(url);
      setFeedback("Handoff registrado. Exporte o artefato no navegador e importe-o abaixo.");
    } catch {
      setFeedback("Não foi possível registrar o handoff para o navegador padrão.");
    } finally {
      setBusyAction(null);
    }
  }

  async function searchEvidence() {
    const query = searchQuery.trim();
    if (!query) {
      setFeedback("Informe termos de busca específicos.");
      return;
    }
    const provider = searchProvider.trim();
    if (!provider) {
      setFeedback("Informe crossref, openalex ou o ID exato de um conector configurado.");
      return;
    }
    setBusyAction("search");
    try {
      const result = await searchWebEvidence({ query, provider, limit: 10 });
      setRecords((current) => mergeEvidence(current, result.items));
      setSelectedEvidenceId(result.items[0]?.id ?? selectedEvidenceId);
      setTotalRecords((current) => Math.max(current, records.length + result.items.length));
      setFeedback(
        `${result.total.toLocaleString("pt-BR")} resultados persistidos pelo provedor ${result.provider}.`,
      );
    } catch {
      setFeedback(
        "A busca não foi concluída. Tente outro provedor ou uma consulta mais específica.",
      );
    } finally {
      setBusyAction(null);
    }
  }

  async function replaySelected() {
    if (!selectedEvidence) return;
    setBusyAction("replay");
    try {
      const record = await replayWebEvidence(selectedEvidence.id);
      storeRecord(record);
      setFeedback(
        "Replay concluído. O detalhe registra se o hash permaneceu igual, mudou ou não estava disponível.",
      );
    } catch {
      setFeedback("O replay falhou. O registro anterior foi preservado sem alteração.");
    } finally {
      setBusyAction(null);
    }
  }

  async function reloadSelected(evidenceId: string) {
    setBusyAction("detail");
    setSelectedEvidenceId(evidenceId);
    try {
      const record = await getWebEvidence(evidenceId);
      setRecords((current) => mergeEvidence(current, [record]));
    } catch {
      setFeedback("Não foi possível atualizar os detalhes desta evidência.");
    } finally {
      setBusyAction(null);
    }
  }

  async function confirmInteractionResolved() {
    if (!selectedEvidence) return;
    setBusyAction("resume");
    try {
      const record = await resumeWebEvidenceInteraction(selectedEvidence.id, true);
      storeRecord(record);
      setFeedback(
        "Interação registrada. Importe o artefato exportado para concluir a captura manual.",
      );
    } catch {
      setFeedback("A interação não foi confirmada; a evidência continua pausada com segurança.");
    } finally {
      setBusyAction(null);
    }
  }

  function chooseCaptureFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0] ?? null;
    event.target.value = "";
    if (!file) return;
    if (file.size > MAX_CAPTURE_BYTES) {
      setCaptureFile(null);
      setFeedback("Arquivo recusado: o limite por captura é 16 MiB.");
      return;
    }
    if (!isAcceptedCapture(file)) {
      setCaptureFile(null);
      setFeedback("Tipo recusado. Use HTML, Markdown, PDF, PNG, JPEG, WebP ou texto puro.");
      return;
    }
    setCaptureFile(file);
    setFeedback(
      `${file.name} selecionado; somente nome, tipo e conteúdo serão enviados, nunca o caminho local.`,
    );
  }

  async function importCapture() {
    if (!captureFile) {
      setFeedback("Selecione um arquivo exportado pelo operador.");
      return;
    }
    if (captureFile.size > MAX_CAPTURE_BYTES || !isAcceptedCapture(captureFile)) {
      setFeedback("O arquivo não atende ao limite de 16 MiB e aos tipos permitidos.");
      return;
    }
    setBusyAction("import");
    try {
      const mediaType = captureMediaType(captureFile);
      if (!mediaType) {
        setFeedback("Não foi possível determinar um tipo permitido para o arquivo.");
        return;
      }
      const record = await importOperatorEvidence({
        url: captureUrl.trim() || null,
        name: captureFile.name,
        media_type: mediaType,
        data_base64: await fileToBase64(captureFile),
        notes: captureNotes.trim() ? [captureNotes.trim()] : [],
      });
      storeRecord(record);
      setCaptureFile(null);
      setCaptureNotes("");
      setFeedback("Artefato importado, hasheado e registrado com proveniência do operador.");
    } catch {
      setFeedback("A importação falhou. O arquivo local não foi alterado.");
    } finally {
      setBusyAction(null);
    }
  }

  return (
    <section className="evidence-workspace" aria-label="Evidências">
      <div className="panel evidence-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Motor mecânico</p>
            <h2>Evidências</h2>
          </div>
          <button
            className={isAuditing ? "secondary-button busy" : "secondary-button"}
            type="button"
            onClick={onAudit}
            disabled={isAuditing}
            aria-busy={isAuditing}
          >
            {isAuditing ? <RefreshCw size={18} /> : <Link2 size={18} />}
            {isAuditing ? "Auditando" : "Auditar links e citações"}
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
              <div className={`link-audit-row ${row.tone}`} key={row.link_id}>
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

      <CitationAuditPanel result={citationAuditResult} isAuditing={isAuditing} />

      <div className="panel evidence-collect-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Web evidence</p>
            <h2>Coletar e reproduzir</h2>
          </div>
          <Globe2 size={20} />
        </div>
        <label className="field-label" htmlFor="evidence-url">
          URL pública
        </label>
        <input
          id="evidence-url"
          className="text-input"
          type="url"
          inputMode="url"
          value={targetUrl}
          onChange={(event) => setTargetUrl(event.target.value)}
          placeholder="https://exemplo.com/fonte"
        />
        <div className="evidence-fetch-options">
          <label>
            Método
            <select
              value={method}
              onChange={(event) => setMethod(event.target.value as WebEvidenceMethod)}
            >
              <option value="GET">GET</option>
              <option value="HEAD">HEAD</option>
            </select>
          </label>
          <label className="check-row evidence-check-row">
            <input
              type="checkbox"
              checked={forceRevalidate}
              onChange={(event) => setForceRevalidate(event.target.checked)}
            />
            Ignorar cache e revalidar
          </label>
        </div>
        <div className="button-row evidence-action-row">
          <button
            className="primary-button"
            type="button"
            disabled={busyAction !== null}
            onClick={() => void collectHttpEvidence()}
          >
            <Play size={17} /> Coletar HTTP
          </button>
          <button
            className="secondary-button"
            type="button"
            disabled={busyAction !== null}
            onClick={() => void collectRenderedEvidence()}
          >
            <MonitorUp size={17} /> Handoff renderizado
          </button>
          <button
            className="secondary-button"
            type="button"
            disabled={busyAction !== null}
            onClick={() => void openInDefaultBrowser()}
          >
            <ExternalLink size={17} /> Navegador padrão
          </button>
        </div>
        <p className="help-text">
          A coleta bloqueia redes privadas e não lê cookies, senhas ou o perfil ativo do navegador.
        </p>

        <div className="evidence-search-form">
          <label className="field-label" htmlFor="evidence-search">
            Busca em fontes configuradas
          </label>
          <input
            id="evidence-search"
            className="text-input"
            type="search"
            value={searchQuery}
            onChange={(event) => setSearchQuery(event.target.value)}
            placeholder="DOI, título, autor ou termos específicos"
          />
          <label>
            Provedor (built-in ou ID configurado)
            <input
              className="text-input"
              type="text"
              list="web-evidence-providers"
              value={searchProvider}
              onChange={(event) => setSearchProvider(event.target.value)}
              placeholder="crossref"
            />
            <datalist id="web-evidence-providers">
              <option value="crossref" />
              <option value="openalex" />
            </datalist>
          </label>
          <button
            className="secondary-button"
            type="button"
            disabled={busyAction !== null}
            onClick={() => void searchEvidence()}
          >
            <Search size={17} /> Buscar
          </button>
        </div>
      </div>

      <LinkIntegrityPanel recentRecords={linkAuditRows} />

      <div className="panel evidence-capture-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Captura do operador</p>
            <h2>Importar artefato</h2>
          </div>
          <FileUp size={20} />
        </div>
        <label className="field-label" htmlFor="capture-url">
          URL de origem
        </label>
        <input
          id="capture-url"
          className="text-input"
          type="url"
          value={captureUrl}
          onChange={(event) => setCaptureUrl(event.target.value)}
          placeholder="Opcional quando a origem não é uma página web"
        />
        <label className="file-drop" htmlFor="evidence-capture-file">
          <FileUp size={22} />
          <strong>{captureFile?.name ?? "Selecionar HTML, Markdown, PDF, imagem ou texto"}</strong>
          <span>Até 16 MiB; somente nome, tipo e conteúdo, nunca o caminho local.</span>
        </label>
        <input
          id="evidence-capture-file"
          className="hidden-input"
          type="file"
          accept=".html,.htm,.md,.markdown,.pdf,.png,.jpg,.jpeg,.webp,.txt,text/html,text/markdown,text/plain,image/png,image/jpeg,image/webp,application/pdf"
          onChange={chooseCaptureFile}
        />
        <label className="field-label" htmlFor="capture-notes">
          Nota de proveniência
        </label>
        <textarea
          id="capture-notes"
          className="text-area evidence-notes"
          value={captureNotes}
          onChange={(event) => setCaptureNotes(event.target.value)}
          placeholder="Descreva o acesso legítimo, a etapa manual ou a licença aplicável. Não cole credenciais."
        />
        <button
          className="primary-button"
          type="button"
          disabled={!captureFile || busyAction !== null}
          onClick={() => void importCapture()}
        >
          <ShieldCheck size={17} /> Importar com proveniência
        </button>
      </div>

      <div className="panel evidence-inventory-panel">
        <div className="panel-heading evidence-inventory-heading">
          <div>
            <p className="eyebrow">Cache persistente</p>
            <h2>Inventário e filas</h2>
          </div>
          <button
            className={busyAction === "inventory" ? "icon-button busy" : "icon-button"}
            type="button"
            aria-label="Atualizar inventário de evidências"
            disabled={busyAction !== null}
            onClick={() => void refreshInventory()}
          >
            <RefreshCw size={18} />
          </button>
        </div>
        <div className="evidence-queue-summary" aria-label="Resumo das filas">
          <span>
            Fila HTTP <strong>{queueSummary.http.toLocaleString("pt-BR")}</strong>
          </span>
          <span>
            Fila de busca <strong>{queueSummary.search.toLocaleString("pt-BR")}</strong>
          </span>
          <span>
            Fila renderizada <strong>{queueSummary.rendered.toLocaleString("pt-BR")}</strong>
          </span>
          <span>
            Prontas <strong>{queueSummary.ready.toLocaleString("pt-BR")}</strong>
          </span>
          <span>
            Expiradas <strong>{queueSummary.stale.toLocaleString("pt-BR")}</strong>
          </span>
          <span>
            Ação humana <strong>{queueSummary.interactive.toLocaleString("pt-BR")}</strong>
          </span>
        </div>
        <div className="evidence-filter-row">
          <input
            className="text-input"
            type="search"
            aria-label="Filtrar inventário de evidências"
            value={filterQuery}
            onChange={(event) => setFilterQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void refreshInventory();
            }}
            placeholder="Filtrar por URL, hash, título ou nota"
          />
          <button
            className="secondary-button"
            type="button"
            disabled={busyAction !== null}
            onClick={() => void refreshInventory()}
          >
            <Search size={17} /> Filtrar
          </button>
        </div>
        <p className="evidence-feedback" role="status" aria-live="polite">
          {feedback}
        </p>
        <div className="evidence-record-list" aria-label="Evidências armazenadas">
          {records.length === 0 && (
            <p className="empty-state">Nenhuma evidência no filtro atual.</p>
          )}
          {records.map((record) => (
            <button
              className={`evidence-record ${record.state} ${selectedEvidenceId === record.id ? "selected" : ""}`}
              type="button"
              key={record.id}
              aria-pressed={selectedEvidenceId === record.id}
              onClick={() => void reloadSelected(record.id)}
            >
              <span className="evidence-record-main">
                <strong>{record.title || record.final_url || record.url}</strong>
                <small>{record.final_url || record.url}</small>
              </span>
              <span className="evidence-record-meta">
                <strong>{stateLabels[record.state]}</strong>
                <small>
                  {record.cache_state} · {formatEvidenceDate(record.retrieved_at)}
                </small>
              </span>
            </button>
          ))}
        </div>
        {nextCursor && (
          <button
            className="secondary-button evidence-more-button"
            type="button"
            disabled={busyAction !== null}
            onClick={() => void refreshInventory(nextCursor)}
          >
            Carregar mais ({records.length.toLocaleString("pt-BR")} de{" "}
            {totalRecords.toLocaleString("pt-BR")})
          </button>
        )}
      </div>

      <div className="panel evidence-detail-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Proveniência verificável</p>
            <h2>Detalhe da evidência</h2>
          </div>
          <Database size={20} />
        </div>
        {!selectedEvidence && <p className="empty-state">Selecione uma evidência do inventário.</p>}
        {selectedEvidence && (
          <>
            <div className={`evidence-state-banner ${selectedEvidence.state}`}>
              <strong>{stateLabels[selectedEvidence.state]}</strong>
              <span>
                {selectedEvidence.access_mode} · cache {selectedEvidence.cache_state}
              </span>
            </div>
            <dl className="detail-list evidence-detail-list">
              <div>
                <dt>Origem</dt>
                <dd>{selectedEvidence.url}</dd>
              </div>
              <div>
                <dt>URL final</dt>
                <dd>{selectedEvidence.final_url ?? "não disponível"}</dd>
              </div>
              <div>
                <dt>HTTP</dt>
                <dd>
                  {selectedEvidence.method} · {selectedEvidence.status ?? "sem resposta"} ·{" "}
                  {selectedEvidence.content_type ?? "tipo desconhecido"}
                </dd>
              </div>
              <div>
                <dt>SHA-256</dt>
                <dd>
                  <code>{selectedEvidence.sha256 ?? "ainda não calculado"}</code>
                </dd>
              </div>
              <div>
                <dt>Coleta</dt>
                <dd>{formatEvidenceDate(selectedEvidence.retrieved_at)}</dd>
              </div>
              <div>
                <dt>Validade</dt>
                <dd>
                  {selectedEvidence.cache_ttl} · expira{" "}
                  {formatEvidenceDate(selectedEvidence.expires_at)}
                </dd>
              </div>
              <div>
                <dt>Tamanho</dt>
                <dd>
                  {formatBytes(selectedEvidence.byte_count)} ·{" "}
                  {selectedEvidence.duration_ms?.toLocaleString("pt-BR") ?? "—"} ms
                </dd>
              </div>
              <div>
                <dt>Robots</dt>
                <dd>{selectedEvidence.robots_state}</dd>
              </div>
              <div>
                <dt>Direitos</dt>
                <dd>{selectedEvidence.copyright_state}</dd>
              </div>
              <div>
                <dt>Interação</dt>
                <dd>
                  {selectedEvidence.interaction_state} · resolução humana{" "}
                  {selectedEvidence.human_resolved ? "confirmada" : "não confirmada"}
                </dd>
              </div>
              {selectedEvidence.provider && (
                <div>
                  <dt>Provedor</dt>
                  <dd>{selectedEvidence.provider}</dd>
                </div>
              )}
              {selectedEvidence.query && (
                <div>
                  <dt>Consulta</dt>
                  <dd>{selectedEvidence.query}</dd>
                </div>
              )}
              {selectedEvidence.artifact_name && (
                <div>
                  <dt>Artefato</dt>
                  <dd>{selectedEvidence.artifact_name}</dd>
                </div>
              )}
            </dl>
            {selectedEvidence.redirect_chain.length > 0 && (
              <details className="evidence-details-disclosure">
                <summary>
                  Redirecionamentos (
                  {selectedEvidence.redirect_chain.length.toLocaleString("pt-BR")})
                </summary>
                <ol>
                  {selectedEvidence.redirect_chain.map((redirect) => (
                    <li key={`${redirect.status}-${redirect.url}`}>
                      {redirect.status} · {redirect.url}
                    </li>
                  ))}
                </ol>
              </details>
            )}
            {selectedEvidence.curl_command && (
              <details className="evidence-details-disclosure">
                <summary>Comando curl reproduzível e saneado</summary>
                <pre>{selectedEvidence.curl_command}</pre>
              </details>
            )}
            {selectedEvidence.notes.length > 0 && (
              <details className="evidence-details-disclosure">
                <summary>Notas ({selectedEvidence.notes.length.toLocaleString("pt-BR")})</summary>
                <ul>
                  {[...new Set(selectedEvidence.notes)].map((note) => (
                    <li key={note}>{note}</li>
                  ))}
                </ul>
              </details>
            )}
            <div className="button-row evidence-action-row">
              <button
                className="secondary-button"
                type="button"
                disabled={busyAction !== null}
                onClick={() => void replaySelected()}
              >
                <RotateCcw size={17} /> Reproduzir coleta
              </button>
              {selectedEvidence.state === "operator_action_required" && (
                <button
                  className="primary-button"
                  type="button"
                  disabled={busyAction !== null}
                  onClick={() => void confirmInteractionResolved()}
                >
                  <Play size={17} /> Confirmar etapa humana
                </button>
              )}
            </div>
          </>
        )}
      </div>

      <div className="panel evidence-progress-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Atividade desta tela</p>
            <h2>Fila em tempo real</h2>
          </div>
          <RefreshCw size={20} />
        </div>
        <div className="evidence-progress-list" aria-live="polite">
          {progressEvents.length === 0 && <p className="empty-state">Nenhuma operação ativa.</p>}
          {progressEvents.map((event) => (
            <div
              className="evidence-progress-row"
              key={`${event.at}-${event.operation}-${event.evidence_id}-${event.phase}-${event.message}`}
            >
              <strong>
                {event.operation} · {event.phase}
              </strong>
              <span>{event.message}</span>
              <small>{formatEvidenceDate(event.at)}</small>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
