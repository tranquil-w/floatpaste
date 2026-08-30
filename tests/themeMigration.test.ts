import test from "node:test";
import assert from "node:assert/strict";
import {
  ACCENT_FOLLOW_PRESET,
  getThemePreset,
  isThemePresetId,
  migrateLegacyThemeAccent,
  nativeWindowTheme,
  resolveAccentHex,
} from "../src/shared/theme/presets.ts";

test("旧自定义强调色（非默认）迁移保留", () => {
  assert.equal(
    migrateLegacyThemeAccent({
      light: { windowBg: "#EFF2F5", cardBg: "#E6EAEF", accent: "#8250DF" },
      dark: { windowBg: "#282C34", cardBg: "#2E333C", accent: "#478BE6" },
    }),
    "#8250DF",
  );
});

test("旧强调色均为默认值时回退跟随预设", () => {
  assert.equal(
    migrateLegacyThemeAccent({
      light: { windowBg: "#EFF2F5", cardBg: "#E6EAEF", accent: "#0969DA" },
      dark: { windowBg: "#282C34", cardBg: "#2E333C", accent: "#478BE6" },
    }),
    ACCENT_FOLLOW_PRESET,
  );
});

test("缺失或非法的旧强调色回退跟随预设", () => {
  assert.equal(migrateLegacyThemeAccent(undefined), ACCENT_FOLLOW_PRESET);
  assert.equal(migrateLegacyThemeAccent({}), ACCENT_FOLLOW_PRESET);
  assert.equal(
    migrateLegacyThemeAccent({ light: { windowBg: "", cardBg: "", accent: "blue" } }),
    ACCENT_FOLLOW_PRESET,
  );
});

test("themeAccent 解析：预设默认 / 安全列表 / 任意 hex / 非法回退", () => {
  const preset = getThemePreset("catppuccin");
  assert.equal(resolveAccentHex("default", preset, "light"), preset.scales.light.accent);
  assert.equal(resolveAccentHex("purple", preset, "light"), "#8e4ec6");
  assert.equal(resolveAccentHex("purple", preset, "dark"), "#9a5cd0");
  assert.equal(resolveAccentHex("#123456", preset, "light"), "#123456");
  assert.equal(resolveAccentHex("#123456", preset, "dark"), "#123456");
  assert.equal(resolveAccentHex("not-a-color", preset, "light"), preset.scales.light.accent);
  assert.equal(resolveAccentHex(undefined, preset, "dark"), preset.scales.dark.accent);
});

test("未知预设 id 回退默认预设", () => {
  assert.equal(getThemePreset("solarized"), getThemePreset("default"));
  assert.equal(isThemePresetId("catppuccin"), true);
  assert.equal(isThemePresetId("solarized"), false);
});

test("跟随系统时原生窗口主题必须清除覆盖，显式模式返回对应值", () => {
  // WebView2 的 prefers-color-scheme 跟随窗口主题：显式设置后 matchMedia 被锁定，
  // "跟随系统"必须传 null 清除覆盖，否则永远解析回上一个显式模式
  assert.equal(nativeWindowTheme("system", "dark"), null);
  assert.equal(nativeWindowTheme("system", "light"), null);
  assert.equal(nativeWindowTheme("light", "light"), "light");
  assert.equal(nativeWindowTheme("dark", "dark"), "dark");
});
