import { ACCENT_CHOICES } from "./accents.ts";
import { PALETTE_SCALES } from "./palettes.ts";
import type { ResolvedTheme, ThemePreset, ThemePresetId } from "./types.ts";

export type { ResolvedTheme, ThemePresetId };

export const DEFAULT_THEME_PRESET: ThemePresetId = "default";

/** themeAccent 字段的取值语义："default"=跟随预设 | 安全列表 id | 迁移保留的 #RRGGBB */
export const ACCENT_FOLLOW_PRESET = "default";

/**
 * 带系统装饰的窗口（settings/editor）应应用的原生主题。
 * "跟随系统"返回 null（清除窗口级覆盖，让 WebView2 的 prefers-color-scheme
 * 恢复跟随系统）；显式亮暗返回对应值。
 */
export function nativeWindowTheme(
  themeMode: "system" | ResolvedTheme,
  resolvedTheme: ResolvedTheme,
): "light" | "dark" | null {
  return themeMode === "system" ? null : resolvedTheme;
}

export const THEME_PRESETS: ThemePreset[] = [
  {
    id: "default",
    name: "默认",
    description: "低色度中性灰基调，跨设备观感最稳定。",
    scales: PALETTE_SCALES.default,
  },
  {
    id: "catppuccin",
    name: "Catppuccin",
    description: "柔和低饱和的社区配色，亮暗为 Latte / Mocha。",
    scales: PALETTE_SCALES.catppuccin,
  },
  {
    id: "tokyoNight",
    name: "Tokyo Night",
    description: "蓝墨夜色风格，亮暗为 day / night。",
    scales: PALETTE_SCALES.tokyoNight,
  },
];

export function isThemePresetId(value: unknown): value is ThemePresetId {
  return THEME_PRESETS.some((preset) => preset.id === value);
}

export function getThemePreset(id: unknown): ThemePreset {
  return THEME_PRESETS.find((preset) => preset.id === id) ?? THEME_PRESETS[0]!;
}

/**
 * 把 themeAccent 字段解析为当前模式下的强调色 hex。
 * 支持 "default"（跟随预设）、安全列表 id、以及旧版迁移保留的 #RRGGBB；
 * 非法输入一律回退预设自带强调色。
 */
export function resolveAccentHex(
  themeAccent: string | undefined,
  preset: ThemePreset,
  resolvedTheme: ResolvedTheme,
): string {
  const value = themeAccent?.trim();
  if (!value || value === ACCENT_FOLLOW_PRESET) {
    return preset.scales[resolvedTheme].accent;
  }

  if (/^#[0-9a-fA-F]{6}$/.test(value)) {
    return value.toUpperCase();
  }

  const choice = ACCENT_CHOICES.find((item) => item.id === value);
  if (choice) {
    return resolvedTheme === "light" ? choice.light : choice.dark;
  }

  return preset.scales[resolvedTheme].accent;
}

/**
 * themeAccent 是否指向某个具体颜色（安全列表或迁移 hex），用于设置页选中态。
 */
export function isExplicitAccent(themeAccent: string | undefined): boolean {
  const value = themeAccent?.trim() ?? "";
  if (!value || value === ACCENT_FOLLOW_PRESET) {
    return false;
  }
  return /^#[0-9a-fA-F]{6}$/.test(value) || ACCENT_CHOICES.some((item) => item.id === value);
}

export interface LegacyCustomThemeColors {
  light?: { windowBg?: string; cardBg?: string; accent?: string };
  dark?: { windowBg?: string; cardBg?: string; accent?: string };
}

/** 旧版两套主题各自的默认强调色；非默认值才会被迁移保留 */
const LEGACY_DEFAULT_ACCENTS = {
  light: "#0969DA",
  dark: "#478BE6",
};

/**
 * 旧 customThemeColors -> 新 themeAccent 的迁移规则（共识 Q3=b）：
 * 底色一律由预设接管；亮/暗强调色中与旧默认不同的合法 hex 作为自定义色保留。
 */
export function migrateLegacyThemeAccent(legacy: LegacyCustomThemeColors | undefined): string {
  const lightAccent = legacy?.light?.accent?.trim().toUpperCase();
  const darkAccent = legacy?.dark?.accent?.trim().toUpperCase();

  if (
    lightAccent &&
    /^#[0-9a-fA-F]{6}$/.test(lightAccent) &&
    lightAccent !== LEGACY_DEFAULT_ACCENTS.light
  ) {
    return lightAccent;
  }
  if (
    darkAccent &&
    /^#[0-9a-fA-F]{6}$/.test(darkAccent) &&
    darkAccent !== LEGACY_DEFAULT_ACCENTS.dark
  ) {
    return darkAccent;
  }
  return ACCENT_FOLLOW_PRESET;
}
