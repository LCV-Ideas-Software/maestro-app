import { Search, ShieldCheck } from "lucide-react";
import { stateIcon, stateLabel } from "../../helpers";
import type { AgentCard, ProtocolReadingGate } from "../../types";

type AgentsScreenProps = {
  agentCards: AgentCard[];
  protocolGateItems: ProtocolReadingGate[];
  sessionRunId: string | null;
  onVerify: () => void;
};

export function AgentsScreen({
  agentCards,
  protocolGateItems,
  sessionRunId,
  onVerify,
}: AgentsScreenProps) {
  return (
    <section className="main-grid" aria-label="Agentes">
      <div className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">{sessionRunId ?? "sem run"}</p>
            <h2>Agentes</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            title="Verificar agentes"
            onClick={onVerify}
          >
            <Search size={18} />
          </button>
        </div>

        <div className="agent-list">
          {agentCards.map((agent) => (
            <div className={`agent-row ${agent.state}`} key={agent.name}>
              <div className="agent-main">
                <div className="agent-icon">{stateIcon(agent.state)}</div>
                <div>
                  <strong>{agent.name}</strong>
                  <span>{agent.cli}</span>
                </div>
              </div>
              <div className="agent-status">
                <strong>{stateLabel(agent.state)}</strong>
                <span>{agent.note}</span>
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="panel reading-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Regra obrigatoria</p>
            <h2>Leitura integral</h2>
          </div>
          <ShieldCheck size={20} />
        </div>
        <div className="reading-list">
          {protocolGateItems.map((gate) => (
            <div className="reading-row" key={gate.agent}>
              <div>
                <strong>{gate.agent}</strong>
                <span>{gate.status}</span>
              </div>
              <div className="mini-progress" aria-label={`${gate.progress}%`}>
                <div style={{ width: `${gate.progress}%` }} />
              </div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
