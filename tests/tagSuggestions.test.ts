import test from "node:test";
import assert from "node:assert/strict";
import { buildTagSuggestions, normalizeTagInput } from "../src/features/editor/tagSuggestions.ts";

const ALL_TAGS = [
  { name: "工作 资料", itemCount: 12 },
  { name: "灵感", itemCount: 8 },
  { name: "代码片段", itemCount: 5 },
  { name: "临时", itemCount: 2 },
];

test("空输入返回常用标签并排除已添加项", () => {
  const suggestions = buildTagSuggestions({
    allTags: ALL_TAGS,
    current: ["灵感"],
    keyword: "",
  });

  assert.deepEqual(suggestions, [
    { kind: "match", name: "工作 资料", itemCount: 12 },
    { kind: "match", name: "代码片段", itemCount: 5 },
    { kind: "match", name: "临时", itemCount: 2 },
  ]);
});

test("输入按包含关系匹配，英文大小写不敏感", () => {
  const suggestions = buildTagSuggestions({
    allTags: [...ALL_TAGS, { name: "TypeScript", itemCount: 3 }],
    current: [],
    keyword: "typescript",
  });

  assert.deepEqual(suggestions, [{ kind: "match", name: "TypeScript", itemCount: 3 }]);

  const partial = buildTagSuggestions({
    allTags: ALL_TAGS,
    current: [],
    keyword: "片段",
  });
  // 库中有"代码片段"但无同名"片段"，匹配项之外仍可创建新标签
  assert.deepEqual(partial, [
    { kind: "match", name: "代码片段", itemCount: 5 },
    { kind: "create", name: "片段" },
  ]);
});

test("输入与已添加标签重复时返回 exists 项并带规范写法", () => {
  const suggestions = buildTagSuggestions({
    allTags: ALL_TAGS,
    current: ["Work"],
    keyword: "work",
  });

  assert.deepEqual(suggestions, [{ kind: "exists", name: "Work" }]);
});

test("无匹配时末尾追加创建项", () => {
  const suggestions = buildTagSuggestions({
    allTags: ALL_TAGS,
    current: [],
    keyword: "  随手记  ",
  });

  assert.deepEqual(suggestions, [{ kind: "create", name: "随手记" }]);
});

test("有匹配时匹配项在前、创建项在后", () => {
  const suggestions = buildTagSuggestions({
    allTags: ALL_TAGS,
    current: [],
    keyword: "代码",
  });

  assert.deepEqual(suggestions, [
    { kind: "match", name: "代码片段", itemCount: 5 },
    { kind: "create", name: "代码" },
  ]);
});

test("建议数量上限为 8 条（不含状态项）", () => {
  const pool = Array.from({ length: 12 }, (_, index) => ({
    name: `标签${index}`,
    itemCount: 12 - index,
  }));
  const suggestions = buildTagSuggestions({ allTags: pool, current: [], keyword: "标签" });

  const matchCount = suggestions.filter((suggestion) => suggestion.kind === "match").length;
  assert.equal(matchCount, 8);
  assert.equal(suggestions[suggestions.length - 1]?.kind, "create");
});

test("normalizeTagInput 折叠空白并去除首尾空格", () => {
  assert.equal(normalizeTagInput("  工作   资料 \t"), "工作 资料");
  assert.equal(normalizeTagInput("   "), "");
});
