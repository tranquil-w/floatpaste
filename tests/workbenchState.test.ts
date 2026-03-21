import test from "node:test";
import assert from "node:assert/strict";
import type { ClipItemDetail, ClipItemSummary } from "../src/shared/types/clips";
import {
  getCachedTextStateForSelection,
  getNextWorkbenchNavigationIndex,
} from "../src/features/workbench/state";

function createSummary(id: string): ClipItemSummary {
  return {
    id,
    contentPreview: `preview-${id}`,
    type: "text",
    createdAt: "2026-03-21T00:00:00Z",
    lastUsedAt: null,
    isFavorited: false,
  };
}

function createDetail(fullText: string): ClipItemDetail {
  return {
    id: "detail-id",
    type: "text",
    sourceApp: null,
    createdAt: "2026-03-21T00:00:00Z",
    fullText,
    isFavorited: false,
  };
}

test("切换到已缓存的文本条目时，立即返回可回填的草稿内容", () => {
  assert.deepEqual(
    getCachedTextStateForSelection(createDetail("新的内容")),
    {
      draftText: "新的内容",
      savedText: "新的内容",
    },
  );
});

test("快速连续向下导航时，基于当前选中项计算下一个索引", () => {
  const items = [createSummary("a"), createSummary("b"), createSummary("c")];

  assert.equal(getNextWorkbenchNavigationIndex(items, "a", "down"), 1);
  assert.equal(getNextWorkbenchNavigationIndex(items, "b", "down"), 2);
});
