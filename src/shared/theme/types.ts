import type { PaletteScale } from "./palettes.ts";

/** 解析后的明暗模式（ThemeMode 去掉 system 之后的两态） */
export type ResolvedTheme = "light" | "dark";

/** 预设 id，与设置里的 themePreset 字段对应 */
export type ThemePresetId = "default" | "catppuccin" | "tokyoNight";

/** 强调色安全列表条目：亮暗各给一个 Radix 基准值，派生时再做对比度校正 */
export interface AccentChoice {
  id: string;
  label: string;
  light: string;
  dark: string;
}

/** 派生输出的语义 token（CSS 变量名 -> 值），键集恒定，组件只允许消费这一层 */
export type SemanticTokens = Record<string, string>;

export interface ThemePreset {
  id: ThemePresetId;
  name: string;
  /** 设置页预设卡片上的一句介绍 */
  description: string;
  scales: Record<ResolvedTheme, PaletteScale>;
}
