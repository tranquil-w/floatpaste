import test from "node:test";
import assert from "node:assert/strict";
import {
  CONTRAST_TARGETS,
  SHADOW_TOKENS,
  deriveSemanticTokens,
  mixColors,
  toCssVariableName,
} from "../src/shared/theme/derive.ts";
import { contrastRatio, ensureContrast, pickForegroundOn } from "../src/shared/theme/contrast.ts";
import { ACCENT_CHOICES } from "../src/shared/theme/accents.ts";
import { THEME_PRESETS, resolveAccentHex } from "../src/shared/theme/presets.ts";

test("派生结果的 token 键集在全预设与模式下保持恒定", () => {
  const expected = new Set(
    Object.keys(deriveSemanticTokens(THEME_PRESETS[0]!.scales.light, "#0090ff", "light")),
  );
  for (const preset of THEME_PRESETS) {
    for (const mode of ["light", "dark"] as const) {
      for (const accent of ["default", ...ACCENT_CHOICES.map((choice) => choice.id)]) {
        const tokens = deriveSemanticTokens(
          preset.scales[mode],
          resolveAccentHex(accent, preset, mode),
          mode,
        );
        assert.deepEqual(new Set(Object.keys(tokens)), expected);
      }
    }
  }
});

test("低对比强调色会被校正到达标且色相基本保留", () => {
  // cyan 亮色在近白底上只有约 3:1，必须被抬升
  const scale = THEME_PRESETS[0]!.scales.light;
  const tokens = deriveSemanticTokens(scale, "#00a2c7", "light");
  const ratio = contrastRatio(tokens["accent-fg"]!, scale.canvas);
  assert.ok(ratio >= CONTRAST_TARGETS.subtle, `accent-fg 未达标: ${ratio}`);
  assert.ok(
    tokens["accent-fg"] !== "#00a2c7",
    "原色不达标时应被校正而不是原样输出",
  );
});

test("已达标的标准强调色不被改动（保官方原味）", () => {
  // 深色底上的亮蓝达标，应原样输出
  const scale = THEME_PRESETS[0]!.scales.dark;
  const tokens = deriveSemanticTokens(scale, "#3b9eff", "dark");
  assert.equal(tokens["accent-fg"], "#3b9eff");
});

test("accent-emphasis 上的前景色永远在黑白中取对比更高者", () => {
  assert.equal(pickForegroundOn("#0090ff", "#1F2328", "#FFFFFF"), "#1F2328");
  assert.equal(pickForegroundOn("#0a3069", "#1F2328", "#FFFFFF"), "#FFFFFF");
});

test("实底强调色与黑白前景均不足时会推移底色亮度", () => {
  // 中亮青色的黑白前景都不到 4.5:1，emphasis 底必须被推移
  const scale = THEME_PRESETS[0]!.scales.light;
  const tokens = deriveSemanticTokens(scale, "#00a2c7", "light");
  const onEmphasis = contrastRatio(tokens["fg-on-emphasis"]!, tokens["accent-emphasis"]!);
  assert.ok(onEmphasis >= CONTRAST_TARGETS.emphasis, `按钮前景对比不足: ${onEmphasis}`);
});

test("ensureContrast 对已达标颜色幂等", () => {
  assert.equal(ensureContrast("#1c2024", "#f9f9fb", 4.5), "#1c2024");
});

test("mixColors 在 OKLab 空间混合且结果落在两端点之间", () => {
  const mixed = mixColors("#000000", "#ffffff", 0.5);
  const lightness = contrastRatio(mixed, "#000000");
  assert.ok(lightness > 1.5 && lightness < 8, `中点混色异常: ${mixed}`);
  assert.equal(mixColors("#335577", "#335577", 0.8), "#335577");
});

test("toCssVariableName 补全 --pg- 前缀", () => {
  assert.equal(toCssVariableName("fg-default"), "--pg-fg-default");
});

test("shadow token 以 shadow-color 内联变量为基底", () => {
  for (const value of Object.values(SHADOW_TOKENS)) {
    assert.match(value, /var\(--pg-(shadow-color|border-default)\)/);
  }
});

test("accent-rgb 通道串与 accent-fg 十六进制一致", () => {
  const scale = THEME_PRESETS[0]!.scales.light;
  const tokens = deriveSemanticTokens(scale, "#8e4ec6", "light");
  const [red, green, blue] = tokens["accent-rgb"]!.split(", ").map((part) =>
    Number.parseInt(part, 10).toString(16).padStart(2, "0"),
  );
  assert.equal(`#${red}${green}${blue}`, tokens["accent-fg"]!.toLowerCase());
});
