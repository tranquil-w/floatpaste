import test from "node:test";
import assert from "node:assert/strict";
import { getActiveSectionId } from "../src/features/settings/settingsScrollSpy.ts";
import { SETTINGS_SECTIONS } from "../src/features/settings/settingsSections.ts";

type Box = { getBoundingClientRect: () => { top: number } };

const SECTION_IDS = SETTINGS_SECTIONS.map((section) => section.id);

function box(top: number): Box {
  return { getBoundingClientRect: () => ({ top }) };
}

function buildSectionBoxes(tops: Partial<Record<string, number>>) {
  return new Map(Object.entries(tops).map(([id, top]) => [id, box(top)]));
}

test("滚动到底时即使末分组未越过偏移线也高亮末分组", () => {
  const sections = buildSectionBoxes({
    general: -500,
    shortcuts: -200,
    tags: 200,
  });

  // scrollY + viewportHeight 恰好触底：tags 顶部仍在偏移线（96px）下方
  const active = getActiveSectionId(SECTION_IDS, sections, {
    scrollY: 900,
    viewportHeight: 760,
    scrollHeight: 1660,
  });

  assert.equal(active, "tags");
});

test("未触底时高亮顶部偏移线经过的最后一个分组", () => {
  const sections = buildSectionBoxes({
    general: -500,
    shortcuts: -200,
    tags: 200,
  });

  const active = getActiveSectionId(SECTION_IDS, sections, {
    scrollY: 300,
    viewportHeight: 760,
    scrollHeight: 3000,
  });

  assert.equal(active, "shortcuts");
});

test("页面顶部所有分组均在偏移线下方时回退首个分组", () => {
  const sections = buildSectionBoxes({
    general: 120,
    shortcuts: 400,
    appearance: 900,
  });

  const active = getActiveSectionId(SECTION_IDS, sections, {
    scrollY: 0,
    viewportHeight: 760,
    scrollHeight: 3000,
  });

  assert.equal(active, "general");
});

test("触底兜底在末分组未注册时退回顶部线所在的分组", () => {
  const sections = buildSectionBoxes({
    general: -600,
    excludedApps: -100,
  });

  const active = getActiveSectionId(SECTION_IDS, sections, {
    scrollY: 900,
    viewportHeight: 760,
    scrollHeight: 1660,
  });

  assert.equal(active, "excludedApps");
});
