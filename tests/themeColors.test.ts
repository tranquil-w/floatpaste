import test from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_CUSTOM_THEME_COLORS,
  buildThemeCssVariables,
  sanitizeCustomThemeColors,
  sanitizeHexColor,
} from "../src/shared/themeColors.ts";

test("sanitizeHexColor 会把非法值回退到默认颜色", () => {
  assert.equal(sanitizeHexColor("blue", "#112233"), "#112233");
  assert.equal(sanitizeHexColor("#12", "#112233"), "#112233");
  assert.equal(sanitizeHexColor("  #aaBBcc ", "#112233"), "#AABBCC");
});

test("sanitizeCustomThemeColors 会分别回退浅色和深色非法值", () => {
  const sanitized = sanitizeCustomThemeColors({
    light: {
      windowBg: "blue",
      cardBg: "#eeffee",
      accent: "#2255cc",
    },
    dark: {
      windowBg: "#111111",
      cardBg: "#22",
      accent: "green",
    },
  });

  assert.equal(sanitized.light.windowBg, DEFAULT_CUSTOM_THEME_COLORS.light.windowBg);
  assert.equal(sanitized.light.cardBg, "#EEFFEE");
  assert.equal(sanitized.dark.windowBg, "#111111");
  assert.equal(sanitized.dark.cardBg, DEFAULT_CUSTOM_THEME_COLORS.dark.cardBg);
  assert.equal(sanitized.dark.accent, DEFAULT_CUSTOM_THEME_COLORS.dark.accent);
});

test("buildThemeCssVariables 会产出 tooltip 和界面共享的运行时变量", () => {
  const vars = buildThemeCssVariables("light", DEFAULT_CUSTOM_THEME_COLORS);

  assert.equal(vars["--pg-canvas-default"], DEFAULT_CUSTOM_THEME_COLORS.light.windowBg);
  assert.equal(vars["--pg-canvas-subtle"], DEFAULT_CUSTOM_THEME_COLORS.light.cardBg);
  assert.equal(vars["--pg-accent-fg"], DEFAULT_CUSTOM_THEME_COLORS.light.accent);
  assert.equal(vars["--pg-accent-rgb"], "9, 105, 218");
  assert.match(vars["--pg-accent-subtle"], /^rgba\(9, 105, 218, 0\.\d+\)$/);
});

test("强调色较亮时按钮前景自动翻转为深色文字", () => {
  const vars = buildThemeCssVariables("light", {
    light: { windowBg: "#FFFFFF", cardBg: "#F0F0F0", accent: "#FFD700" },
    dark: { windowBg: "#111111", cardBg: "#222222", accent: "#478BE6" },
  });

  assert.equal(vars["--pg-fg-on-emphasis"], "#1F2328");
});

test("默认强调色保持白色按钮文字", () => {
  const light = buildThemeCssVariables("light", DEFAULT_CUSTOM_THEME_COLORS);
  const dark = buildThemeCssVariables("dark", DEFAULT_CUSTOM_THEME_COLORS);

  assert.equal(light["--pg-fg-on-emphasis"], "#FFFFFF");
  assert.equal(dark["--pg-fg-on-emphasis"], "#FFFFFF");
});

test("默认配色不派生前景/中性色变量，保持 Primer 观感", () => {
  const light = buildThemeCssVariables("light", DEFAULT_CUSTOM_THEME_COLORS);
  const dark = buildThemeCssVariables("dark", DEFAULT_CUSTOM_THEME_COLORS);

  assert.equal("--pg-fg-default" in light, false);
  assert.equal("--pg-fg-default" in dark, false);
  assert.equal("--pg-neutral-3" in light, false);
});

test("自定义深色底配浅色主题时，前景文字翻转为浅色保证可读", () => {
  const vars = buildThemeCssVariables("light", {
    light: { windowBg: "#2E333C", cardBg: "#282C34", accent: "#0969DA" },
    dark: { windowBg: "#111111", cardBg: "#222222", accent: "#478BE6" },
  });

  assert.equal(vars["--pg-fg-default"], "#E8ECF2");
  // muted/subtle 由浅色文字向深底混合得到，仍是浅色调
  assert.ok(vars["--pg-fg-muted"].startsWith("#"));
  assert.notEqual(vars["--pg-fg-muted"], "#59636E");
  assert.equal("--pg-neutral-3" in vars, true);
});

test("自定义浅色底配深色主题时，前景文字保持深色保证可读", () => {
  const vars = buildThemeCssVariables("dark", {
    light: { windowBg: "#EFF2F5", cardBg: "#E6EAEF", accent: "#0969DA" },
    dark: { windowBg: "#F5F5F5", cardBg: "#EAEAEA", accent: "#478BE6" },
  });

  assert.equal(vars["--pg-fg-default"], "#1F2328");
});

test("仅自定义强调色时不派生前景色，正文观感不变", () => {
  const vars = buildThemeCssVariables("light", {
    light: {
      windowBg: DEFAULT_CUSTOM_THEME_COLORS.light.windowBg,
      cardBg: DEFAULT_CUSTOM_THEME_COLORS.light.cardBg,
      accent: "#8250DF",
    },
    dark: DEFAULT_CUSTOM_THEME_COLORS.dark,
  });

  assert.equal("--pg-fg-default" in vars, false);
  assert.equal(vars["--pg-accent-fg"], "#8250DF");
});
