import { Database } from "lucide-react";
import { navGroups, storageModeSummaries } from "../constants";
import type { ActiveSection, CredentialStorageMode } from "../types";

type AppSidebarProps = {
  activeSection: ActiveSection;
  appVersion: string;
  credentialStorageMode: CredentialStorageMode;
  onChooseSection: (section: ActiveSection) => void;
};

export function AppSidebar({
  activeSection,
  appVersion,
  credentialStorageMode,
  onChooseSection,
}: AppSidebarProps) {
  return (
    <aside className="sidebar">
      <div className="brand">
        <div className="brand-mark">M</div>
        <div>
          <div className="brand-name">Maestro Editorial AI</div>
          <div className="brand-meta">{appVersion}</div>
        </div>
      </div>

      <nav className="nav-list" aria-label="Principal">
        {navGroups.map((group) => (
          <div className="nav-group" key={group.label}>
            <div className="nav-group-label">{group.label}</div>
            {group.items.map((item) => {
              const Icon = item.icon;
              return (
                <button
                  className={activeSection === item.section ? "nav-item active" : "nav-item"}
                  type="button"
                  key={item.section}
                  aria-current={activeSection === item.section ? "page" : undefined}
                  onClick={() => onChooseSection(item.section)}
                >
                  <Icon size={18} />
                  {item.label}
                </button>
              );
            })}
          </div>
        ))}
      </nav>

      <div className="storage-strip">
        <Database size={18} />
        <div>
          <strong>{storageModeSummaries[credentialStorageMode].title}</strong>
          <span>{storageModeSummaries[credentialStorageMode].detail}</span>
        </div>
      </div>
    </aside>
  );
}
