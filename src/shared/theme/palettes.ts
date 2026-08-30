import type { ResolvedTheme } from "./types.ts";

/**
 * 预设在单一明暗模式下的角色化色板（语义中间层）。
 *
 * 约定：
 * - 浅色模式：surface 比浅（或等于）canvas 更"沉"，即更深的灰阶；
 *   深色模式相反，surface 比 canvas 更亮表示浮起。
 * - 文字/边框/强调/状态色给官方配色原值作为起点，
 *   派生层会用 ensureContrast 校正到可读，不必在这里手工压深。
 * - 新增预设时沿用本接口，逐字段对照官方色板填写，并在注释注明出处。
 */
export interface PaletteScale {
  /** 窗口主体底色 */
  canvas: string;
  /** 分区、悬浮条、列表 hover 等"次级底" */
  canvasSubtle: string;
  /** 卡片、输入框、面板底 */
  surface: string;
  /** 下沉区域（输入内衬、代码块、徽章底） */
  inset: string;
  /** 正文 */
  ink: string;
  /** 次级正文 */
  inkMuted: string;
  /** 弱文字、占位符（仍须可读，由门禁保证 ≥4.5:1） */
  inkSubtle: string;
  /** 承载结构的边框（卡片描边、输入框），门禁 ≥3:1 */
  border: string;
  /** 弱分隔线，纯装饰、无门禁 */
  borderMuted: string;
  /** 预设自带强调色（themeAccent = "default" 时使用） */
  accent: string;
  /** 状态色基调 */
  success: string;
  danger: string;
  warning: string;
  done: string;
  /** 收藏星标（填充图形，无文字对比门禁） */
  favorite: string;
}

/* ── 默认预设：Radix Colors slate 中性阶 + 色相阶 ──
 * 出处：@radix-ui/colors 3.x（slate.css / slate-dark.css 及各色相 css），MIT。
 * slate 在深色下的蓝调色度极低（C≈0.004），夜间模式色温偏移下远比旧
 * Primer dark 稳定；文字阶与强调色走 9/10/11/12 阶的 Radix 语义惯例。
 */
const defaultLight: PaletteScale = {
  canvas: "#f9f9fb",
  canvasSubtle: "#f0f0f3",
  surface: "#ffffff",
  inset: "#f0f0f3",
  ink: "#1c2024",
  inkMuted: "#5c6168",
  inkSubtle: "#6d727b",
  border: "#8b8d98",
  borderMuted: "#d9d9e0",
  accent: "#0090ff",
  success: "#30a46c",
  danger: "#e5484d",
  warning: "#f76b15",
  done: "#8e4ec6",
  favorite: "#f76b15",
};

const defaultDark: PaletteScale = {
  canvas: "#18191b",
  canvasSubtle: "#212225",
  surface: "#272a2d",
  inset: "#111113",
  ink: "#edeef0",
  inkMuted: "#b0b4ba",
  inkSubtle: "#7f838c",
  border: "#61676f",
  borderMuted: "#363a3f",
  accent: "#3b9eff",
  success: "#33b074",
  danger: "#ec5d5e",
  warning: "#ff801f",
  done: "#9a5cd0",
  favorite: "#ff801f",
};

/* ── Catppuccin Latte / Mocha ──
 * 出处：catppuccin/palette v1.8（官方 palette.json），MIT。
 * Latte 沿用官方"浮层比底色深一档"的层级（crust 作卡片底）；
 * Mocha 用 surface0 作浮起卡片、mantle/crust 作下沉层。
 */
const catppuccinLight: PaletteScale = {
  canvas: "#eff1f5",
  canvasSubtle: "#e6e9ef",
  surface: "#ffffff",
  inset: "#e6e9ef",
  ink: "#4c4f69",
  inkMuted: "#5c5f77",
  inkSubtle: "#6c6f85",
  border: "#878aa0",
  borderMuted: "#ccd0da",
  accent: "#1e66f5",
  success: "#40a02b",
  danger: "#d20f39",
  warning: "#df8e1d",
  done: "#8839ef",
  favorite: "#df8e1d",
};

const catppuccinDark: PaletteScale = {
  canvas: "#1e1e2e",
  canvasSubtle: "#181825",
  surface: "#313244",
  inset: "#11111b",
  ink: "#cdd6f4",
  inkMuted: "#a6adc8",
  inkSubtle: "#868aa2",
  border: "#6e7288",
  borderMuted: "#45475a",
  accent: "#89b4fa",
  success: "#a6e3a1",
  danger: "#f38ba8",
  warning: "#f9e2af",
  done: "#cba6f7",
  favorite: "#f9e2af",
};

/* ── Tokyo Night day / night ──
 * 出处：folke/tokyonight.nvim extras/tailwindv4（官方 hex），MIT。
 * day 沿用其"蓝墨正文"特征色（fg #3760bf），可读性由派生校正兜底。
 */
const tokyoNightLight: PaletteScale = {
  canvas: "#e1e2e7",
  canvasSubtle: "#d0d5e3",
  surface: "#ffffff",
  inset: "#c4c8da",
  ink: "#3760bf",
  inkMuted: "#6172b0",
  inkSubtle: "#848cb5",
  border: "#848cb5",
  borderMuted: "#c4c8da",
  accent: "#2e7de9",
  success: "#587539",
  danger: "#f52a65",
  warning: "#b15c00",
  done: "#9854f1",
  favorite: "#b15c00",
};

const tokyoNightDark: PaletteScale = {
  canvas: "#1a1b26",
  canvasSubtle: "#16161e",
  surface: "#292e42",
  inset: "#0c0e14",
  ink: "#c0caf5",
  inkMuted: "#a9b1d6",
  inkSubtle: "#7d85a8",
  border: "#5e688f",
  borderMuted: "#343a52",
  accent: "#7aa2f7",
  success: "#9ece6a",
  danger: "#f7768e",
  warning: "#e0af68",
  done: "#9d7cd8",
  favorite: "#e0af68",
};

export const PALETTE_SCALES: Record<string, Record<ResolvedTheme, PaletteScale>> = {
  default: { light: defaultLight, dark: defaultDark },
  catppuccin: { light: catppuccinLight, dark: catppuccinDark },
  tokyoNight: { light: tokyoNightLight, dark: tokyoNightDark },
};
