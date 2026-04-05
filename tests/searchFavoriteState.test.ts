import test from "node:test";
import assert from "node:assert/strict";
import type {
  ClipItemDetail,
  ClipItemSummary,
  SearchResult,
} from "../src/shared/types/clips.ts";
import {
  getSearchItemFavoritedState,
  setFavoritedOnDetail,
  setFavoritedOnSearchResult,
} from "../src/features/search/favoritedState.ts";

function createSummary(isFavorited: boolean): ClipItemSummary {
  return {
    id: "clip-1",
    type: "text",
    contentPreview: "hello",
    tooltipText: null,
    sourceApp: "FloatPaste",
    isFavorited,
    fileCount: 0,
    directoryCount: 0,
    createdAt: "2026-04-06T00:00:00.000Z",
    updatedAt: "2026-04-06T00:00:00.000Z",
    lastUsedAt: null,
    imagePath: null,
    imageWidth: null,
    imageHeight: null,
    imageFormat: null,
    fileSize: null,
  };
}

function createDetail(isFavorited: boolean): ClipItemDetail {
  return {
    id: "clip-1",
    type: "text",
    contentPreview: "hello",
    sourceApp: "FloatPaste",
    isFavorited,
    createdAt: "2026-04-06T00:00:00.000Z",
    updatedAt: "2026-04-06T00:00:00.000Z",
    lastUsedAt: null,
    searchText: "hello",
    hash: "hash",
    fullText: "hello",
    imagePath: null,
    imageWidth: null,
    imageHeight: null,
    imageFormat: null,
    fileSize: null,
    filePaths: [],
    fileCount: 0,
    directoryCount: 0,
    totalSize: null,
  };
}

test("搜索窗口应优先使用当前列表项的收藏状态，避免被旧详情覆盖", () => {
  assert.equal(
    getSearchItemFavoritedState(createSummary(true), createDetail(false)),
    true,
  );
});

test("setFavoritedOnSearchResult 会更新命中的列表项收藏状态", () => {
  const result: SearchResult = {
    items: [createSummary(false)],
    total: 1,
    offset: 0,
    limit: 50,
  };

  const next = setFavoritedOnSearchResult(result, "clip-1", true);

  assert.equal(next?.items[0]?.isFavorited, true);
  assert.equal(result.items[0]?.isFavorited, false);
});

test("只看收藏时取消收藏会立刻把条目从结果中移除", () => {
  const result: SearchResult = {
    items: [createSummary(true)],
    total: 1,
    offset: 0,
    limit: 50,
  };

  const next = setFavoritedOnSearchResult(result, "clip-1", false, {
    removeUnfavoritedItem: true,
  });

  assert.equal(next?.items.length, 0);
  assert.equal(next?.total, 0);
});

test("setFavoritedOnDetail 会同步更新当前详情缓存", () => {
  const next = setFavoritedOnDetail(createDetail(false), "clip-1", true);

  assert.equal(next?.isFavorited, true);
});
