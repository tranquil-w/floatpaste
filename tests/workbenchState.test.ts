import test from "node:test";
import assert from "node:assert/strict";
import type { ClipItemSummary } from "../src/shared/types/clips";
import { getNextWorkbenchNavigationIndex } from "../src/features/workbench/state";

function createSummary(id: string): ClipItemSummary {
  return {
    id,
    contentPreview: `preview-${id}`,
    type: "text",
    sourceApp: null,
    isFavorited: false,
    fileCount: 0,
    directoryCount: 0,
    createdAt: "2026-03-21T00:00:00Z",
    updatedAt: "2026-03-21T00:00:00Z",
    lastUsedAt: null,
  };
}

test("快速连续向下导航时，仍基于当前选中项计算下一个索引", () => {
  const items = [createSummary("a"), createSummary("b"), createSummary("c")];

  assert.equal(getNextWorkbenchNavigationIndex(items, "a", "down"), 1);
  assert.equal(getNextWorkbenchNavigationIndex(items, "b", "down"), 2);
});
