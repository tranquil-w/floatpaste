// 主题数值校验脚本：跑全预设 × 模式 × accent 的派生结果，输出关键对比度。
// 用法：node scripts/verify-theme.ts（调色或新增预设后先跑一遍再进测试）
import { deriveSemanticTokens, CONTRAST_TARGETS } from "../src/shared/theme/derive.ts";
import { contrastRatio } from "../src/shared/theme/contrast.ts";
import { ACCENT_CHOICES } from "../src/shared/theme/accents.ts";
import { THEME_PRESETS, resolveAccentHex } from "../src/shared/theme/presets.ts";
import type { ResolvedTheme } from "../src/shared/theme/types.ts";

let failures = 0;
function check(label: string, fg: string, bg: string, target: number) {
  const ratio = contrastRatio(fg, bg);
  if (ratio < target) {
    failures += 1;
    console.log(`  ✗ ${label}: ${fg} on ${bg} = ${ratio.toFixed(2)} < ${target}`);
  }
  return ratio;
}

for (const preset of THEME_PRESETS) {
  for (const mode of ["light", "dark"] as ResolvedTheme[]) {
    const scale = preset.scales[mode];
    const accents = ["default", ...ACCENT_CHOICES.map((c) => c.id)];
    for (const accentId of accents) {
      const accentHex = resolveAccentHex(accentId, preset, mode);
      const t = deriveSemanticTokens(scale, accentHex, mode);
      const label = `${preset.id}/${mode}/${accentId}`;
      if (accentId === "default") {
        console.log(`\n== ${preset.id} ${mode} ==`);
        console.log(
          [
            `ink ${t["fg-default"]}(${contrastRatio(t["fg-default"], scale.canvas).toFixed(1)})`,
            `muted ${t["fg-muted"]}(${contrastRatio(t["fg-muted"], scale.canvas).toFixed(1)})`,
            `subtle ${t["fg-subtle"]}(${contrastRatio(t["fg-subtle"], scale.canvas).toFixed(1)})`,
            `border ${t["border-default"]}(${contrastRatio(t["border-default"], scale.canvas).toFixed(1)})`,
            `accentFg ${t["accent-fg"]}`,
            `emphasis ${t["accent-emphasis"]}(onEmphasis ${t["fg-on-emphasis"]})`,
            `success ${t["success-fg"]} danger ${t["danger-fg"]} warning ${t["warning-fg"]} done ${t["done-fg"]}`,
          ].join("\n  "),
        );
      }
      check(`${label} fg-default`, t["fg-default"], scale.canvas, CONTRAST_TARGETS.body);
      check(`${label} fg-muted`, t["fg-muted"], scale.canvas, CONTRAST_TARGETS.body);
      check(`${label} fg-subtle`, t["fg-subtle"], scale.canvas, CONTRAST_TARGETS.subtle);
      check(`${label} accent-fg`, t["accent-fg"], scale.canvas, CONTRAST_TARGETS.subtle);
      check(`${label} on-emphasis`, t["fg-on-emphasis"], t["accent-emphasis"], CONTRAST_TARGETS.emphasis);
      check(`${label} border`, t["border-default"], scale.canvas, CONTRAST_TARGETS.border);
      check(`${label} success`, t["success-fg"], scale.canvas, CONTRAST_TARGETS.subtle);
      check(`${label} danger`, t["danger-fg"], scale.canvas, CONTRAST_TARGETS.subtle);
      check(`${label} warning`, t["warning-fg"], scale.canvas, CONTRAST_TARGETS.subtle);
      check(`${label} done`, t["done-fg"], scale.canvas, CONTRAST_TARGETS.subtle);
    }
  }
}

console.log(failures === 0 ? "\nALL PASS" : `\n${failures} FAILURES`);
