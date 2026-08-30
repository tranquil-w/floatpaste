import { clampChroma, clampRgb, formatHex, interpolate, oklch, parse } from "culori";
import type { Color } from "culori";
import { contrastRatio, ensureContrast, hexToRgbChannels } from "./contrast.ts";
import type { PaletteScale } from "./palettes.ts";
import type { ResolvedTheme, SemanticTokens } from "./types.ts";

/** 实底强调色上的深色前景（黑用柔和近黑而非纯黑，观感与旧版一致） */
const DARK_INK_ON_EMPHASIS = "#1F2328";
const LIGHT_INK_ON_EMPHASIS = "#FFFFFF";

/** 门禁常量：正文类 AA+ 余量、组件边界非文本线；对比度测试与派生共用 */
export const CONTRAST_TARGETS = {
  body: 5.5,
  subtle: 4.5,
  emphasis: 4.5,
  border: 3,
} as const;

const SUBTLE_ALPHA = { light: 0.13, dark: 0.19 } as const;
const STATUS_SUBTLE_ALPHA = { light: 0.13, dark: 0.16 } as const;

function toHex(color: Color): string {
  return formatHex(clampRgb(clampChroma(color, "oklch"))) ?? "#000000";
}

/** OKLab 插值混色，取代旧版 sRGB 通道线性混合（跨屏观感漂移的根源之一） */
export function mixColors(base: string, target: string, ratio: number): string {
  const from = parse(base) ?? { mode: "rgb" as const, r: 0, g: 0, b: 0 };
  const to = parse(target) ?? { mode: "rgb" as const, r: 0, g: 0, b: 0 };
  return toHex(interpolate([from, to], "oklab")(ratio));
}

/** 实底强调色：黑白前景都无法达到 4.5:1 时，向改动更小的方向推移底色亮度 */
function ensureEmphasisBackground(accent: string): { emphasis: string; fgOnEmphasis: string } {
  const white = contrastRatio(LIGHT_INK_ON_EMPHASIS, accent);
  const black = contrastRatio(DARK_INK_ON_EMPHASIS, accent);
  if (Math.max(white, black) >= CONTRAST_TARGETS.emphasis) {
    return {
      emphasis: accent,
      fgOnEmphasis: black >= white ? DARK_INK_ON_EMPHASIS : LIGHT_INK_ON_EMPHASIS,
    };
  }

  const darkerForWhiteInk = ensureContrast(
    accent,
    LIGHT_INK_ON_EMPHASIS,
    CONTRAST_TARGETS.emphasis,
  );
  const lighterForDarkInk = ensureContrast(accent, DARK_INK_ON_EMPHASIS, CONTRAST_TARGETS.emphasis);
  const accentL = oklch(accent)?.l ?? 0.5;
  const darkerL = oklch(darkerForWhiteInk)?.l ?? 0;
  const lighterL = oklch(lighterForDarkInk)?.l ?? 1;
  const preferDarker = Math.abs(accentL - darkerL) <= Math.abs(lighterL - accentL);
  const emphasis = preferDarker ? darkerForWhiteInk : lighterForDarkInk;
  return {
    emphasis,
    fgOnEmphasis: preferDarker ? LIGHT_INK_ON_EMPHASIS : DARK_INK_ON_EMPHASIS,
  };
}

/**
 * 从角色化色板 + 强调色派生全套语义 token。
 *
 * 规则：
 * - 文字/边框/accent 状态色先取官方原值，不足门禁的经 OKLCH 亮度校正达标
 *   （官方配色观感优先，校正只在必要时介入且幂等）；
 * - accent-fg 是"底上的强调文字"（校正 ≥4.5），accent-emphasis 是
 *   "实底按钮底"（保持饱和，前景黑白自动二选一，不足时推移底色）；
 * - subtle 底一律由对应前景色的 rgba 生成，明暗模式各用一组 alpha。
 */
export function deriveSemanticTokens(
  scale: PaletteScale,
  accentHex: string,
  resolvedTheme: ResolvedTheme,
): SemanticTokens {
  const isLight = resolvedTheme === "light";
  const subtleAlpha = isLight ? SUBTLE_ALPHA.light : SUBTLE_ALPHA.dark;
  const statusAlpha = isLight ? STATUS_SUBTLE_ALPHA.light : STATUS_SUBTLE_ALPHA.dark;

  const ink = ensureContrast(scale.ink, scale.canvas, CONTRAST_TARGETS.body);
  const inkMuted = ensureContrast(scale.inkMuted, scale.canvas, CONTRAST_TARGETS.body);
  const inkSubtle = ensureContrast(scale.inkSubtle, scale.canvas, CONTRAST_TARGETS.subtle);
  const border = ensureContrast(scale.border, scale.canvas, CONTRAST_TARGETS.border);

  const accentFg = ensureContrast(accentHex, scale.canvas, CONTRAST_TARGETS.subtle);
  const { emphasis, fgOnEmphasis } = ensureEmphasisBackground(accentHex);
  const accentHover = mixColors(emphasis, isLight ? "#000000" : "#ffffff", isLight ? 0.12 : 0.14);

  const statusTokens = (base: string, fgVar: string, emphasisVar: string, subtleVar: string) => {
    const fg = ensureContrast(base, scale.canvas, CONTRAST_TARGETS.subtle);
    return {
      [fgVar]: fg,
      [emphasisVar]: base,
      [subtleVar]: `rgba(${hexToRgbChannels(fg)}, ${statusAlpha})`,
    };
  };

  return {
    "canvas-default": scale.canvas,
    "canvas-subtle": scale.canvasSubtle,
    "canvas-inset": scale.inset,

    "fg-default": ink,
    "fg-muted": inkMuted,
    "fg-subtle": inkSubtle,
    "fg-on-emphasis": fgOnEmphasis,

    "accent-fg": accentFg,
    "accent-emphasis": emphasis,
    "accent-hover": accentHover,
    "accent-subtle": `rgba(${hexToRgbChannels(accentFg)}, ${subtleAlpha})`,
    "accent-rgb": hexToRgbChannels(accentFg),

    "border-default": border,
    "border-window": mixColors(scale.borderMuted, scale.canvas, 0.55),
    "border-muted": scale.borderMuted,
    "border-subtle": mixColors(scale.borderMuted, scale.canvas, 0.45),
    "border-accent": accentFg,

    ...statusTokens(scale.success, "success-fg", "success-emphasis", "success-subtle"),
    ...statusTokens(scale.danger, "danger-fg", "danger-emphasis", "danger-subtle"),
    ...statusTokens(scale.warning, "warning-fg", "warning-emphasis", "warning-subtle"),
    ...statusTokens(scale.done, "done-fg", "done-emphasis", "done-subtle"),

    favorite: scale.favorite,

    // 阴影 = 加深边缘：浅色用墨色，深色必须用黑色——
    // 若沿用深色下的亮色墨色，窗口阴影会变成一圈白光，看起来像发亮的边框
    "shadow-color": isLight ? hexToRgbChannels(ink) : "0, 0, 0",
  };
}

/** 派生结果的 CSS 变量名前缀；写入 root.style 时统一加前缀 */
export const TOKEN_VAR_PREFIX = "--pg-";

export function toCssVariableName(tokenKey: string): string {
  return `${TOKEN_VAR_PREFIX}${tokenKey}`;
}

export const SHADOW_TOKENS: Record<string, string> = {
  "shadow-sm": "0 1px 0 var(--pg-border-default)",
  "shadow-md": "0 3px 6px rgba(var(--pg-shadow-color), 0.04)",
  "shadow-lg": "0 8px 24px rgba(var(--pg-shadow-color), 0.12)",
  "shadow-xl":
    "0 12px 28px rgba(var(--pg-shadow-color), 0.12), 0 2px 4px rgba(var(--pg-shadow-color), 0.08)",
};
