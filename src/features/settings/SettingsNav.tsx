import { SETTINGS_SECTIONS, type SettingsSectionId } from "./settingsSections";

type SettingsNavProps = {
  activeSectionId: SettingsSectionId;
  layoutMode: "sidebar" | "compact";
  onSelect: (id: SettingsSectionId) => void;
};

export function SettingsNav({ activeSectionId, layoutMode, onSelect }: SettingsNavProps) {
  if (layoutMode === "compact") {
    return (
      <nav aria-label="设置分组" className="mb-6">
        <div className="flex flex-wrap gap-2 rounded-xl border border-pg-border-muted bg-pg-canvas-subtle p-2">
          {SETTINGS_SECTIONS.map((section) => {
            const isActive = section.id === activeSectionId;

            return (
              <button
                aria-current={isActive ? "true" : undefined}
                className={`rounded-lg px-3 py-2 text-sm transition-colors ${
                  isActive
                    ? "border border-pg-accent-fg bg-pg-accent-subtle text-pg-fg-default"
                    : "border border-transparent text-pg-fg-muted hover:border-pg-border-default hover:bg-pg-canvas-default hover:text-pg-fg-default"
                }`}
                key={section.id}
                onClick={() => onSelect(section.id)}
                type="button"
              >
                {section.label}
              </button>
            );
          })}
        </div>
      </nav>
    );
  }

  return (
    <nav aria-label="设置分组" className="sticky top-8 self-start">
      <div className="rounded-2xl border border-pg-border-muted bg-pg-canvas-subtle p-3 shadow-sm">
        <div className="px-2 pb-3">
          <p className="text-[11px] font-semibold uppercase tracking-[0.16em] text-pg-fg-subtle">
            偏好设置
          </p>
        </div>
        <div className="space-y-1">
          {SETTINGS_SECTIONS.map((section) => {
            const isActive = section.id === activeSectionId;

            return (
              <button
                aria-current={isActive ? "true" : undefined}
                className={`w-full rounded-xl border px-3 py-3 text-left transition-colors ${
                  isActive
                    ? "border-pg-accent-fg bg-pg-accent-subtle text-pg-fg-default"
                    : "border-transparent text-pg-fg-muted hover:border-pg-border-default hover:bg-pg-canvas-default hover:text-pg-fg-default"
                }`}
                key={section.id}
                onClick={() => onSelect(section.id)}
                type="button"
              >
                <span className="block text-sm font-medium">{section.label}</span>
                <span className="mt-1 block text-xs text-pg-fg-subtle">
                  {section.description}
                </span>
              </button>
            );
          })}
        </div>
      </div>
    </nav>
  );
}
