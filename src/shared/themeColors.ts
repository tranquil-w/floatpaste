import type { CustomThemeColors, ThemeColorPalette, ThemeMode } from "./types/settings";

export type ResolvedTheme = Exclude<ThemeMode, "system">;

export const DEFAULT_CUSTOM_THEME_COLORS: CustomThemeColors = {
  light: {
    windowBg: "#EFF2F5",
    cardBg: "#E6EAEF",
    accent: "#0969DA",
  },
  dark: {
    windowBg: "#282C34",
    cardBg: "#2E333C",
    accent: "#478BE6",
  },
};

const HEX_COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;

function mixHexColors(base: string, target: string, ratio: number): string {
  const clampedRatio = Math.max(0, Math.min(1, ratio));
  const left = parseHexColor(base);
  const right = parseHexColor(target);
  const mixed = left.map((value, index) =>
    Math.round(value + (right[index]! - value) * clampedRatio)
  );

  return toHexColor(mixed[0]!, mixed[1]!, mixed[2]!);
}

function parseHexColor(value: string): [number, number, number] {
  return [
    Number.parseInt(value.slice(1, 3), 16),
    Number.parseInt(value.slice(3, 5), 16),
    Number.parseInt(value.slice(5, 7), 16),
  ];
}

function toHexColor(red: number, green: number, blue: number): string {
  return `#${[red, green, blue].map((value) => value.toString(16).padStart(2, "0")).join("").toUpperCase()}`;
}

function toRgbChannels(value: string): string {
  const [red, green, blue] = parseHexColor(value);
  return `${red}, ${green}, ${blue}`;
}

function toRgba(value: string, alpha: number): string {
  return `rgba(${toRgbChannels(value)}, ${alpha})`;
}

function sanitizePalette(
  palette: ThemeColorPalette | undefined,
  fallback: ThemeColorPalette,
): ThemeColorPalette {
  return {
    windowBg: sanitizeHexColor(palette?.windowBg, fallback.windowBg),
    cardBg: sanitizeHexColor(palette?.cardBg, fallback.cardBg),
    accent: sanitizeHexColor(palette?.accent, fallback.accent),
  };
}

export function sanitizeHexColor(value: string | undefined, fallback: string): string {
  const trimmed = value?.trim() ?? "";
  return HEX_COLOR_PATTERN.test(trimmed) ? trimmed.toUpperCase() : fallback;
}

export function sanitizeCustomThemeColors(
  colors: CustomThemeColors | undefined,
): CustomThemeColors {
  return {
    light: sanitizePalette(colors?.light, DEFAULT_CUSTOM_THEME_COLORS.light),
    dark: sanitizePalette(colors?.dark, DEFAULT_CUSTOM_THEME_COLORS.dark),
  };
}

export function getCustomThemeColorErrors(colors: CustomThemeColors): Record<string, string> {
  const sanitized = sanitizeCustomThemeColors(colors);
  const errors: Record<string, string> = {};

  for (const [themeName, palette] of Object.entries(colors) as Array<[keyof CustomThemeColors, ThemeColorPalette]>) {
    const next = sanitized[themeName];
    for (const [fieldName, value] of Object.entries(palette) as Array<[keyof ThemeColorPalette, string]>) {
      if (value.trim() && next[fieldName] !== value.trim().toUpperCase()) {
        errors[`${themeName}.${fieldName}`] = "请输入 #RRGGBB 格式的十六进制颜色";
      }
    }
  }

  return errors;
}

export function buildThemeCssVariables(
  resolvedTheme: ResolvedTheme,
  colors: CustomThemeColors | undefined,
): Record<string, string> {
  const palette = sanitizeCustomThemeColors(colors)[resolvedTheme];
  const isLightTheme = resolvedTheme === "light";
  const accentRgb = toRgbChannels(palette.accent);
  const canvasInset = mixHexColors(
    palette.windowBg,
    isLightTheme ? "#FFFFFF" : "#111111",
    isLightTheme ? 0.18 : 0.28,
  );
  const borderDefault = mixHexColors(
    palette.cardBg,
    isLightTheme ? "#000000" : "#FFFFFF",
    isLightTheme ? 0.14 : 0.18,
  );
  const borderMuted = mixHexColors(
    palette.cardBg,
    isLightTheme ? "#000000" : "#FFFFFF",
    isLightTheme ? 0.1 : 0.13,
  );
  const borderSubtle = mixHexColors(
    palette.cardBg,
    palette.windowBg,
    0.4,
  );
  const accentHover = mixHexColors(
    palette.accent,
    "#FFFFFF",
    isLightTheme ? 0.08 : 0.12,
  );

  return {
    "--pg-user-window-bg": palette.windowBg,
    "--pg-user-card-bg": palette.cardBg,
    "--pg-user-accent": palette.accent,
    "--pg-user-accent-rgb": accentRgb,
    "--pg-canvas-default": palette.windowBg,
    "--pg-canvas-subtle": palette.cardBg,
    "--pg-canvas-inset": canvasInset,
    "--pg-border-default": borderDefault,
    "--pg-border-muted": borderMuted,
    "--pg-border-subtle": borderSubtle,
    "--pg-border-accent": palette.accent,
    "--pg-accent-fg": palette.accent,
    "--pg-accent-emphasis": palette.accent,
    "--pg-accent-hover": accentHover,
    "--pg-accent-subtle": toRgba(palette.accent, isLightTheme ? 0.14 : 0.18),
    "--pg-accent-rgb": accentRgb,
    "--pg-blue-5": palette.accent,
    "--pg-blue-4": accentHover,
    "--pg-blue-5-rgb": accentRgb,
  };
}
