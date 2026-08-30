import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  SHADOW_TOKENS,
  deriveSemanticTokens,
  toCssVariableName,
} from "../src/shared/theme/derive.ts";
import { THEME_PRESETS, resolveAccentHex } from "../src/shared/theme/presets.ts";

const repoRoot = fileURLToPath(new URL("..", import.meta.url));
const indexCss = readFileSync(`${repoRoot}src/index.css`, "utf8");

function extractVarBlock(css: string, selector: string): Map<string, string> {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`${escaped}\\s*\\{([\\s\\S]*?)\\}`));
  assert.ok(match, `index.css 中找不到 ${selector} 块`);
  const vars = new Map<string, string>();
  for (const varMatch of match[1]!.matchAll(/(--pg-[a-z0-9-]+):\s*([^;]*);/g)) {
    vars.set(varMatch[1]!, varMatch[2]!.replace(/\s+/g, " ").trim());
  }
  return vars;
}

function expectedTokens(mode: "light" | "dark"): Map<string, string> {
  const preset = THEME_PRESETS[0]!;
  const scale = preset.scales[mode];
  const tokens = deriveSemanticTokens(scale, resolveAccentHex("default", preset, mode), mode);
  const expected = new Map<string, string>();
  for (const [key, value] of Object.entries(tokens)) {
    expected.set(toCssVariableName(key), value.toLowerCase());
  }
  // 阴影 token 亮暗共用同一组值（引用 --pg-shadow-color 随模式解析），仅在 :root 定义
  if (mode === "light") {
    for (const [key, value] of Object.entries(SHADOW_TOKENS)) {
      expected.set(toCssVariableName(key), value.replace(/\s+/g, " ").trim());
    }
  }
  return expected;
}

function assertBlocksMatch(actual: Map<string, string>, expected: Map<string, string>, label: string) {
  for (const [name, value] of expected) {
    assert.ok(
      actual.has(name),
      `${label} 缺少兜底变量 ${name}；index.css 静态兜底需与派生层同步`,
    );
    assert.equal(
      actual.get(name)!.toLowerCase(),
      value,
      `${label} 中 ${name} 与派生结果不一致：刷新 index.css 兜底块`,
    );
  }
}

test("index.css 的 :root 兜底与默认预设浅色派生完全一致", () => {
  assertBlocksMatch(extractVarBlock(indexCss, ":root"), expectedTokens("light"), ":root");
});

test("index.css 的 html.dark 兜底与默认预设深色派生完全一致", () => {
  assertBlocksMatch(extractVarBlock(indexCss, "html.dark"), expectedTokens("dark"), "html.dark");
});
