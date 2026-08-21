import { FileText, Link2, Upload } from "lucide-react";
import type { ChangeEventHandler, RefObject } from "react";
import { contentPipelines, importChannels } from "../../constants";
import type { ProtocolSnapshot } from "../../types";

type ProtocolsScreenProps = {
  inputRef: RefObject<HTMLInputElement | null>;
  protocol: ProtocolSnapshot;
  onImport: ChangeEventHandler<HTMLInputElement>;
};

export function ProtocolsScreen({ inputRef, protocol, onImport }: ProtocolsScreenProps) {
  return (
    <section className="main-grid" aria-label="Protocolos">
      <div className="panel protocol-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Biblioteca</p>
            <h2>Protocolo ativo</h2>
          </div>
          <button
            className="secondary-button"
            type="button"
            onClick={() => inputRef.current?.click()}
          >
            <Upload size={18} />
            Importar
          </button>
          <input
            ref={inputRef}
            className="hidden-input"
            type="file"
            accept=".md,text/markdown,text/plain"
            onChange={onImport}
          />
        </div>

        <div className="protocol-record">
          <div className="file-badge">
            <FileText size={26} />
          </div>
          <div>
            <strong>{protocol.name}</strong>
            <span>
              {protocol.size
                ? `${protocol.size.toLocaleString("pt-BR")} bytes`
                : "artefato fonte local"}
            </span>
          </div>
        </div>

        <dl className="detail-list">
          <div>
            <dt>Hash</dt>
            <dd>{protocol.hash}</dd>
          </div>
          <div>
            <dt>Linhas</dt>
            <dd>{protocol.lines.toLocaleString("pt-BR")}</dd>
          </div>
          <div>
            <dt>Publicacao</dt>
            <dd>bloqueada ate unanimidade</dd>
          </div>
        </dl>
      </div>

      <div className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Chats compartilhados</p>
            <h2>Entrada externa</h2>
          </div>
          <Link2 size={20} />
        </div>
        <div className="connector-list">
          {importChannels.map((channel) => (
            <div className="connector-row" key={channel.provider}>
              <strong>{channel.provider}</strong>
              <span>{channel.pattern}</span>
              <em>{channel.status}</em>
            </div>
          ))}
        </div>
      </div>

      <div className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Arquivos</p>
            <h2>Importar e exportar</h2>
          </div>
          <FileText size={20} />
        </div>
        <div className="pipeline-list">
          {contentPipelines.map((pipeline) => (
            <div className="pipeline-row" key={pipeline.label}>
              <span>{pipeline.label}</span>
              <strong>{pipeline.value}</strong>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
