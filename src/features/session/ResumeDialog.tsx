import { EyeOff } from "lucide-react";
import type { ProtocolSnapshot, ResumableSessionInfo } from "../../types";

type ResumeDialogProps = {
  candidates: ResumableSessionInfo[];
  hasLoadedProtocol: boolean;
  protocol: ProtocolSnapshot;
  useLoadedProtocol: boolean;
  formatActivity: (session: ResumableSessionInfo) => string;
  onClose: () => void;
  onChoose: (session: ResumableSessionInfo, useLoadedProtocol: boolean) => void;
  onUseLoadedProtocolChange: (value: boolean) => void;
};

export function ResumeDialog({
  candidates,
  hasLoadedProtocol,
  protocol,
  useLoadedProtocol,
  formatActivity,
  onClose,
  onChoose,
  onUseLoadedProtocolChange,
}: ResumeDialogProps) {
  return (
    <div className="modal-backdrop" role="presentation">
      <section
        className="resume-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Retomar sessao"
      >
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Retomar</p>
            <h2>Escolha uma sessao</h2>
          </div>
          <button className="icon-button" type="button" onClick={onClose} title="Fechar">
            <EyeOff size={18} />
          </button>
        </div>

        <label
          className={
            hasLoadedProtocol ? "resume-protocol-option" : "resume-protocol-option disabled"
          }
        >
          <input
            type="checkbox"
            checked={useLoadedProtocol && hasLoadedProtocol}
            disabled={!hasLoadedProtocol}
            onChange={(event) => onUseLoadedProtocolChange(event.target.checked)}
          />
          <span>
            {hasLoadedProtocol
              ? `Usar protocolo carregado agora: ${protocol.name}`
              : "Usar o protocolo salvo dentro de cada sessao"}
          </span>
        </label>

        <div className="resume-list">
          {candidates.map((session) => (
            <button
              className="resume-session-row"
              type="button"
              key={session.run_id}
              onClick={() => onChoose(session, useLoadedProtocol)}
            >
              <div>
                <strong>{session.session_name}</strong>
                <span>{session.run_id}</span>
              </div>
              <div>
                <strong>Rodada {session.next_round.toLocaleString("pt-BR")}</strong>
                <span>{formatActivity(session)}</span>
              </div>
              <div>
                <strong>{session.status}</strong>
                <span>
                  {session.artifact_count.toLocaleString("pt-BR")} arquivos;{" "}
                  {session.protocol_lines.toLocaleString("pt-BR")} linhas
                </span>
              </div>
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}
