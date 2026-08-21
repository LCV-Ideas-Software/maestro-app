import { Clock3, Play, RefreshCw, Square } from "lucide-react";

type AppTopbarProps = {
  activeLabel: string;
  sessionName: string;
  isResumeLoading: boolean;
  isRunPreparing: boolean;
  isStopRequested: boolean;
  runActionLabel: string;
  sessionRunId: string | null;
  onSessionNameChange: (value: string) => void;
  onRevalidate: () => void;
  onRequestResume: () => void;
  onStart: () => void;
  onStop: () => void;
};

export function AppTopbar({
  activeLabel,
  sessionName,
  isResumeLoading,
  isRunPreparing,
  isStopRequested,
  runActionLabel,
  sessionRunId,
  onSessionNameChange,
  onRevalidate,
  onRequestResume,
  onStart,
  onStop,
}: AppTopbarProps) {
  return (
    <header className="topbar">
      <div>
        <p className="eyebrow">{activeLabel}</p>
        <input
          className="session-title"
          value={sessionName}
          onChange={(event) => onSessionNameChange(event.target.value)}
          aria-label="Nome da sessao"
        />
      </div>
      <div className="toolbar">
        <button className="icon-button" type="button" title="Revalidar" onClick={onRevalidate}>
          <RefreshCw size={18} />
        </button>
        <button
          className={isResumeLoading ? "secondary-button busy" : "secondary-button"}
          type="button"
          onClick={onRequestResume}
          aria-busy={isResumeLoading}
          disabled={isRunPreparing || isResumeLoading}
        >
          <Clock3 size={18} />
          {isResumeLoading ? "Buscando" : "Retomar"}
        </button>
        <button
          className={isRunPreparing ? "primary-button busy" : "primary-button"}
          type="button"
          onClick={onStart}
          aria-busy={isRunPreparing}
          disabled={isRunPreparing}
        >
          <Play size={18} />
          {isRunPreparing ? "Preparando" : runActionLabel}
        </button>
        {isRunPreparing && sessionRunId && (
          <button
            type="button"
            className="secondary-button"
            onClick={onStop}
            disabled={isStopRequested}
            aria-busy={isStopRequested}
            title="Para a sessao em andamento (CLI peer cancela em ~250ms; API peer cancela em <2s)."
          >
            <Square size={18} />
            {isStopRequested ? "Parando…" : "Parar sessao"}
          </button>
        )}
      </div>
    </header>
  );
}
