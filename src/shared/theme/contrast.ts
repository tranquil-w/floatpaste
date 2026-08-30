import { clampChroma, formatHex, oklch, parse, wcagContrast } from "culori";

/**
 * WCAG 2.x 对比度（1 起，21 封顶）。入参为任意 CSS 颜色字符串，解析失败返回 1。
 * 主题门禁与派生校正共用本函数，保证测试口径与运行时行为一致。
 */
export function contrastRatio(foreground: string, background: string): number {
  const fg = parse(foreground);
  const bg = parse(background);
  if (!fg || !bg) {
    return 1;
  }
  return wcagContrast(fg, bg);
}

/**
 * 在 OKLCH 空间只调亮度（保持色相与色度），返回与原色最近、
 * 且相对 against 的 WCAG 对比度达到 target 的颜色。
 * 已达标时原样返回；因色域裁剪导致亮度非严格单调时由二分收敛兜底。
 */
export function ensureContrast(color: string, against: string, target: number): string {
  const base = oklch(color);
  if (!base || contrastRatio(color, against) >= target) {
    return formatHex(parse(color) ?? { mode: "rgb", r: 0, g: 0, b: 0 }) ?? "#000000";
  }

  const againstL = oklch(against)?.l ?? 0.5;
  // 色比底浅则向更亮找，比底深则向更暗找；WCAG 亮度随 OKLCH L 单调，可二分
  const ascending = base.l >= againstL;
  let lo = ascending ? base.l : 0;
  let hi = ascending ? 1 : base.l;

  let best = toHex({ ...base, l: hi === 1 ? 1 : 0 });
  for (let i = 0; i < 24; i += 1) {
    const mid = (lo + hi) / 2;
    const candidate = toHex({ ...base, l: mid });
    if (contrastRatio(candidate, against) >= target) {
      best = candidate;
      if (ascending) {
        hi = mid;
      } else {
        lo = mid;
      }
    } else if (ascending) {
      lo = mid;
    } else {
      hi = mid;
    }
  }
  return best;
}

/**
 * 强调色/状态色实底（按钮、徽章）上的前景色：黑白中取对比更高的一档。
 */
export function pickForegroundOn(background: string, darkInk: string, lightInk: string): string {
  return contrastRatio(darkInk, background) >= contrastRatio(lightInk, background)
    ? darkInk
    : lightInk;
}

/** hex -> "r, g, b" 通道串，供 rgba(var(--pg-accent-rgb), a) 用法 */
export function hexToRgbChannels(hex: string): string {
  const rgb = parse(hex);
  if (!rgb || rgb.mode !== "rgb") {
    return "0, 0, 0";
  }
  return `${Math.round(rgb.r * 255)}, ${Math.round(rgb.g * 255)}, ${Math.round(rgb.b * 255)}`;
}

function toHex(color: ReturnType<typeof oklch> & object): string {
  const clamped = clampChroma(color, "oklch");
  return formatHex(clamped) ?? "#000000";
}
