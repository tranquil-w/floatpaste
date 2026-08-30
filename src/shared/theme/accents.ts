import type { AccentChoice } from "./types.ts";

/**
 * 强调色安全列表（8 档）。出处：Radix Colors 3.x 对应色相的
 * light step 9 / dark step 10（Radix 暗色 solid 推荐阶）。
 *
 * 刻意不收录黄色系：黄色在浅底上无法达到 WCAG AA 文字对比度，
 * 强行校正会失去色相特征。派生层会对这里给出的值再做校正，
 * 因此本表只需保证"色相正确、观感饱和"。
 */
export const ACCENT_CHOICES: AccentChoice[] = [
  { id: "blue", label: "蓝", light: "#0090ff", dark: "#3b9eff" },
  { id: "indigo", label: "靛蓝", light: "#3e63dd", dark: "#5472e4" },
  { id: "cyan", label: "青", light: "#00a2c7", dark: "#23afd0" },
  { id: "green", label: "绿", light: "#30a46c", dark: "#33b074" },
  { id: "orange", label: "橙", light: "#f76b15", dark: "#ff801f" },
  { id: "red", label: "红", light: "#e5484d", dark: "#ec5d5e" },
  { id: "purple", label: "紫", light: "#8e4ec6", dark: "#9a5cd0" },
  { id: "pink", label: "粉", light: "#d6409f", dark: "#de51a8" },
];

export const ACCENT_CHOICE_IDS = ACCENT_CHOICES.map((choice) => choice.id);
