import {
  ACCENT_CHOICES,
  ACCENT_FOLLOW_PRESET,
  THEME_PRESETS,
  getThemePreset,
  resolveAccentHex,
  resolveSemanticTokens,
} from "../../shared/theme";
import type { ResolvedTheme, ThemePresetId } from "../../shared/theme";

/**
 * 预设选择器：用真实派生 token 渲染迷你界面片段，所见即所得。
 * 预览跟随当前生效的明暗模式；选中态用 accent 描边。
 */
export function ThemePresetPicker({
  resolvedTheme,
  themeAccent,
  themePreset,
  onSelectPreset,
}: {
  resolvedTheme: ResolvedTheme;
  themeAccent: string;
  themePreset: ThemePresetId;
  onSelectPreset: (id: ThemePresetId) => void;
}) {
  return (
    <div className="grid gap-2.5 sm:grid-cols-3">
      {THEME_PRESETS.map((preset) => {
        const tokens = resolveSemanticTokens(preset.id, themeAccent, resolvedTheme);
        const selected = themePreset === preset.id;
        return (
          <button
            aria-pressed={selected}
            className={`rounded-xl border p-2 text-left transition-colors ${
              selected
                ? "border-pg-accent-fg bg-pg-canvas-default"
                : "border-pg-border-muted hover:border-pg-border-default"
            }`}
            key={preset.id}
            onClick={() => onSelectPreset(preset.id)}
            type="button"
          >
            <span
              className="block rounded-lg p-2"
              style={{ backgroundColor: tokens["canvas-default"] }}
            >
              <span
                className="block rounded-md border p-1.5"
                style={{
                  backgroundColor: tokens["canvas-subtle"],
                  borderColor: tokens["border-muted"],
                }}
              >
                <span
                  className="block h-1.5 w-4/5 rounded-full"
                  style={{ backgroundColor: tokens["fg-default"] }}
                />
                <span
                  className="mt-1 block h-1.5 w-3/5 rounded-full"
                  style={{ backgroundColor: tokens["fg-muted"] }}
                />
                <span
                  className="mt-1.5 inline-block h-3.5 w-10 rounded-[4px]"
                  style={{ backgroundColor: tokens["accent-emphasis"] }}
                >
                  <span
                    className="mx-1.5 mt-[6px] block h-1 rounded-full"
                    style={{ backgroundColor: tokens["fg-on-emphasis"] }}
                  />
                </span>
              </span>
            </span>
            <span
              className={`mt-1.5 block text-sm font-medium ${
                selected ? "text-pg-fg-default" : "text-pg-fg-muted"
              }`}
            >
              {preset.name}
            </span>
            <span className="mt-0.5 block text-xs leading-relaxed text-pg-fg-subtle">
              {preset.description}
            </span>
          </button>
        );
      })}
    </div>
  );
}

/** 强调色选择器：8 档安全列表 + "跟随预设"档；色点为当前明暗模式下的实际值 */
export function ThemeAccentPicker({
  resolvedTheme,
  themeAccent,
  themePreset,
  onSelectAccent,
}: {
  resolvedTheme: ResolvedTheme;
  themeAccent: string;
  themePreset: ThemePresetId;
  onSelectAccent: (accent: string) => void;
}) {
  const presetAccent = resolveAccentHex(
    ACCENT_FOLLOW_PRESET,
    getThemePreset(themePreset),
    resolvedTheme,
  );
  const options = [
    { id: ACCENT_FOLLOW_PRESET, label: "跟随预设", hex: presetAccent },
    ...ACCENT_CHOICES.map((choice) => ({
      id: choice.id,
      label: choice.label,
      hex: resolvedTheme === "light" ? choice.light : choice.dark,
    })),
  ];
  // 旧版迁移保留的自定义 hex 不在列表里：显示为无选中态，点任一档即覆盖
  const selectedId = options.some((option) => option.id === themeAccent) ? themeAccent : null;

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {options.map((option) => {
        const selected = selectedId === option.id;
        return (
          <button
            aria-label={option.label}
            aria-pressed={selected}
            className={`flex h-7 w-7 items-center justify-center rounded-full border-2 transition-transform hover:scale-105 ${
              selected ? "border-pg-accent-fg" : "border-transparent"
            }`}
            key={option.id}
            onClick={() => onSelectAccent(option.id)}
            title={option.label}
            type="button"
          >
            <span
              className="block h-5 w-5 rounded-full border"
              style={{ backgroundColor: option.hex, borderColor: "rgba(0, 0, 0, 0.08)" }}
            />
          </button>
        );
      })}
    </div>
  );
}
