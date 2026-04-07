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
