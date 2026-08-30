import { SETTINGS_SECTIONS, type SettingsSectionId } from "./settingsSections";

type SettingsNavProps = {
  activeSectionId: SettingsSectionId;
  layoutMode: "sidebar" | "compact";
  onSelect: (id: SettingsSectionId) => void;
};

/**
 * 设置分组导航：单行项 + 活动指示条。
 * 分组描述只在右侧 section 标题区出现一次，导航不重复展示。
 */
export function SettingsNav({ activeSectionId, layoutMode, onSelect }: SettingsNavProps) {
  if (layoutMode === "compact") {
    return (
      <nav aria-label="设置分组" className="mb-4">
        <div className="flex flex-wrap gap-1">
          {SETTINGS_SECTIONS.map((section) => {
            const isActive = section.id === activeSectionId;

            return (
              <button
                aria-current={isActive ? "true" : undefined}
                className={`rounded-lg px-3 py-1.5 text-sm transition-colors ${
                  isActive
                    ? "bg-pg-accent-subtle font-medium text-pg-fg-default"
                    : "text-pg-fg-muted hover:bg-pg-canvas-subtle hover:text-pg-fg-default"
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
    <nav aria-label="设置分组" className="sticky top-16 self-start">
      <p className="px-3 pb-2 text-xs font-medium text-pg-fg-subtle">偏好设置</p>
      <div className="space-y-0.5">
        {SETTINGS_SECTIONS.map((section) => {
          const isActive = section.id === activeSectionId;

          return (
            <button
              aria-current={isActive ? "true" : undefined}
              className={`relative flex w-full items-center rounded-md px-3 py-1.5 text-left text-sm transition-colors ${
                isActive
                  ? "bg-pg-accent-subtle font-medium text-pg-fg-default"
                  : "text-pg-fg-muted hover:bg-pg-canvas-subtle hover:text-pg-fg-default"
              }`}
              key={section.id}
              onClick={() => onSelect(section.id)}
              type="button"
            >
              {isActive ? (
                <span
                  aria-hidden="true"
                  className="absolute left-0 top-1/2 h-4 w-[3px] -translate-y-1/2 rounded-full bg-pg-accent-fg"
                />
              ) : null}
              {section.label}
            </button>
          );
        })}
      </div>
    </nav>
  );
}
