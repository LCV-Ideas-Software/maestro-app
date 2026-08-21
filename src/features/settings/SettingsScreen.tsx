import type { ReactNode } from "react";
import { settingsTabs } from "../../constants";
import type { SettingsTab } from "../../types";

type SettingsScreenProps = {
  activeTab: SettingsTab;
  cloudflarePanel: ReactNode;
  providersPanel: ReactNode;
  onChooseTab: (tab: SettingsTab) => void;
};

export function SettingsScreen({
  activeTab,
  cloudflarePanel,
  providersPanel,
  onChooseTab,
}: SettingsScreenProps) {
  return (
    <section className="settings-layout" aria-label="Configuracoes operacionais">
      <aside className="panel settings-nav-panel" aria-label="Areas de configuracao">
        <div>
          <p className="eyebrow">Configuracoes</p>
          <h2>Ajustes do Maestro</h2>
        </div>
        <div className="settings-tabs">
          {settingsTabs.map((item) => {
            const Icon = item.icon;
            return (
              <button
                className={activeTab === item.tab ? "active" : ""}
                key={item.tab}
                type="button"
                aria-pressed={activeTab === item.tab}
                onClick={() => onChooseTab(item.tab)}
              >
                <Icon size={18} />
                <span>
                  <strong>{item.label}</strong>
                  <em>{item.detail}</em>
                </span>
              </button>
            );
          })}
        </div>
      </aside>

      <div className="settings-content">
        {activeTab === "cloudflare" && cloudflarePanel}
        {activeTab === "providers" && providersPanel}
      </div>
    </section>
  );
}
