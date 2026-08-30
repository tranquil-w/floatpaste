import test from "node:test";
import assert from "node:assert/strict";
import { contrastRatio } from "../src/shared/theme/contrast.ts";
import { CONTRAST_TARGETS, deriveSemanticTokens } from "../src/shared/theme/derive.ts";
import { ACCENT_CHOICES } from "../src/shared/theme/accents.ts";
import { THEME_PRESETS, resolveAccentHex } from "../src/shared/theme/presets.ts";
import type { ResolvedTheme } from "../src/shared/theme/types.ts";
import type { SemanticTokens } from "../src/shared/theme/types.ts";

/**
 * 对比度门禁：全预设 × 明暗模式 × 全部强调色（含预设自带与迁移 hex 样本）。
 * 不达标的组合直接 fail，防止调色时悄悄引入低对比 token。
 */
const ACCENT_SAMPLES = [
  "default",
  ...ACCENT_CHOICES.map((choice) => choice.id),
  // 旧版迁移保留的任意 hex 也必须能被派生校正到达标
  "#123456",
  "#ABCDEF",
];

const MODES: ResolvedTheme[] = ["light", "dark"];

function contrastOf(tokens: SemanticTokens, fgKey: string, bg: string): number {
  return contrastRatio(tokens[fgKey]!, bg);
}

test("全部主题组合的正文与次级文字达到 AA+ 余量", () => {
  for (const preset of THEME_PRESETS) {
    for (const mode of MODES) {
      const canvas = preset.scales[mode].canvas;
      for (const accent of ACCENT_SAMPLES) {
        const tokens = deriveSemanticTokens(
          preset.scales[mode],
          resolveAccentHex(accent, preset, mode),
          mode,
        );
        const label = `${preset.id}/${mode}/${accent}`;

        assert.ok(
          contrastOf(tokens, "fg-default", canvas) >= CONTRAST_TARGETS.body,
          `${label} fg-default 低于 ${CONTRAST_TARGETS.body}:1`,
        );
        assert.ok(
          contrastOf(tokens, "fg-muted", canvas) >= CONTRAST_TARGETS.body,
          `${label} fg-muted 低于 ${CONTRAST_TARGETS.body}:1`,
        );
        assert.ok(
          contrastOf(tokens, "fg-subtle", canvas) >= CONTRAST_TARGETS.subtle,
          `${label} fg-subtle 低于 ${CONTRAST_TARGETS.subtle}:1`,
        );
      }
    }
  }
});

test("全部主题组合的强调色文字与状态色达到 AA", () => {
  for (const preset of THEME_PRESETS) {
    for (const mode of MODES) {
      const canvas = preset.scales[mode].canvas;
      for (const accent of ACCENT_SAMPLES) {
        const tokens = deriveSemanticTokens(
          preset.scales[mode],
          resolveAccentHex(accent, preset, mode),
          mode,
        );
        const label = `${preset.id}/${mode}/${accent}`;

        for (const fgKey of [
          "accent-fg",
          "success-fg",
          "danger-fg",
          "warning-fg",
          "done-fg",
        ] as const) {
          assert.ok(
            contrastOf(tokens, fgKey, canvas) >= CONTRAST_TARGETS.subtle,
            `${label} ${fgKey} 低于 ${CONTRAST_TARGETS.subtle}:1`,
          );
        }
      }
    }
  }
});

test("全部主题组合的实底强调色与边框达到非文本对比线", () => {
  for (const preset of THEME_PRESETS) {
    for (const mode of MODES) {
      const canvas = preset.scales[mode].canvas;
      for (const accent of ACCENT_SAMPLES) {
        const tokens = deriveSemanticTokens(
          preset.scales[mode],
          resolveAccentHex(accent, preset, mode),
          mode,
        );
        const label = `${preset.id}/${mode}/${accent}`;

        assert.ok(
          contrastRatio(tokens["fg-on-emphasis"]!, tokens["accent-emphasis"]!) >=
            CONTRAST_TARGETS.emphasis,
          `${label} 按钮文字对比低于 ${CONTRAST_TARGETS.emphasis}:1`,
        );
        assert.ok(
          contrastOf(tokens, "border-default", canvas) >= CONTRAST_TARGETS.border,
          `${label} border-default 低于 ${CONTRAST_TARGETS.border}:1`,
        );
      }
    }
  }
});

test("深色模式正文对比度保持高冗余（夜间模式/劣屏衰减后仍可读）", () => {
  for (const preset of THEME_PRESETS) {
    const canvas = preset.scales.dark.canvas;
    const tokens = deriveSemanticTokens(
      preset.scales.dark,
      resolveAccentHex("default", preset, "dark"),
      "dark",
    );
    assert.ok(
      contrastOf(tokens, "fg-default", canvas) >= 9,
      `${preset.id} 深色正文对比不足 9:1`,
    );
  }
});
