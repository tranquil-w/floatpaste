import test, { beforeEach } from "node:test";
import assert from "node:assert/strict";
import type { InfiniteData } from "@tanstack/react-query";
import { queryClient } from "../src/app/queryClient.ts";
import { applyClipsChanged } from "../src/shared/queries/clipsCache.ts";
import { queryKeys } from "../src/shared/queries/queryKeys.ts";
import type {
  ClipItemSummary,
  SearchQuery,
  SearchResult,
} from "../src/shared/types/clips.ts";

function createItem(id: string, overrides: Partial<ClipItemSummary> = {}): ClipItemSummary {
  return {
    id,
    type: "text",
    contentPreview: `preview-${id}`,
    sourceApp: "FloatPaste",
    isFavorited: false,
    fileCount: 0,
    directoryCount: 0,
    createdAt: "2026-04-04T00:00:00.000Z",
    updatedAt: "2026-04-04T00:00:00.000Z",
    lastUsedAt: null,
    imagePath: null,
    imageWidth: null,
    imageHeight: null,
    imageFormat: null,
    fileSize: null,
    ...overrides,
  };
}

function createPage(
  ids: string[],
  total: number,
  offset: number,
  limit = 50,
): SearchResult {
  return {
    items: ids.map((id) => createItem(id)),
    total,
    offset,
    limit,
  };
}

function createRecentKey(filters: SearchQuery["filters"] = {}): readonly unknown[] {
  return queryKeys.searchRecent({
    keyword: "",
    filters,
    offset: 0,
    limit: 50,
    sort: "recent_desc",
  });
}

function createSearchKey(keyword: string): readonly unknown[] {
  return queryKeys.searchQuery({
    keyword,
    filters: {},
    offset: 0,
    limit: 50,
    sort: "relevance_desc",
  });
}

function readInfiniteData(key: readonly unknown[]): InfiniteData<SearchResult> {
  const data = queryClient.getQueryData<InfiniteData<SearchResult>>(key);
  assert.ok(data, "缓存应存在分页数据");
  return data;
}

beforeEach(() => {
  queryClient.clear();
});

test("applyClipsChanged 删除时从 search-recent 分页缓存移除条目并同步 total", () => {
  const key = createRecentKey();
  queryClient.setQueryData<InfiniteData<SearchResult>>(key, {
    pages: [createPage(["a", "b"], 3, 0), createPage(["c"], 3, 2)],
    pageParams: [0, 2],
  });

  applyClipsChanged({ kind: "deleted", id: "c" });

  const data = readInfiniteData(key);
  assert.deepEqual(
    data.pages.map((page) => page.items.map((item) => item.id)),
    [["a", "b"], []],
  );
  assert.deepEqual(
    data.pages.map((page) => page.total),
    [2, 2],
  );
});

test("applyClipsChanged 删除时同步更新 search-query 分页缓存", () => {
  const key = createSearchKey("关键词");
  queryClient.setQueryData<InfiniteData<SearchResult>>(key, {
    pages: [createPage(["a", "b", "c"], 3, 0)],
    pageParams: [0],
  });

  applyClipsChanged({ kind: "deleted", id: "b" });

  const data = readInfiniteData(key);
  assert.deepEqual(
    data.pages[0].items.map((item) => item.id),
    ["a", "c"],
  );
  assert.equal(data.pages[0].total, 2);
});

test("applyClipsChanged 删除不存在的条目时保持缓存引用不变", () => {
  const key = createRecentKey();
  const original: InfiniteData<SearchResult> = {
    pages: [createPage(["a"], 1, 0)],
    pageParams: [0],
  };
  queryClient.setQueryData(key, original);

  applyClipsChanged({ kind: "deleted", id: "missing" });

  assert.equal(queryClient.getQueryData(key), original);
});

test("applyClipsChanged 删除时同步清理 picker-recent 数组缓存", () => {
  queryClient.setQueryData(queryKeys.pickerRecent(8), [createItem("a"), createItem("b")]);

  applyClipsChanged({ kind: "deleted", id: "a" });

  assert.deepEqual(
    (queryClient.getQueryData(queryKeys.pickerRecent(8)) as ClipItemSummary[]).map(
      (item) => item.id,
    ),
    ["b"],
  );
});

test("applyClipsChanged upserted 把新条目插入 search-recent 第一页头部并累加 total", () => {
  const key = createRecentKey();
  queryClient.setQueryData<InfiniteData<SearchResult>>(key, {
    pages: [createPage(["a", "b"], 2, 0)],
    pageParams: [0],
  });

  applyClipsChanged({ kind: "upserted", item: createItem("new") });

  const data = readInfiniteData(key);
  // 新条目插头后按页大小截断（与 limit 语义一致），尾部条目交给 refetch 校正
  assert.deepEqual(
    data.pages[0].items.map((item) => item.id),
    ["new", "a"],
  );
  assert.equal(data.pages[0].total, 3);
});

test("applyClipsChanged upserted 激活深处旧条目时置顶第一页并从旧页移除", () => {
  const key = createRecentKey();
  const refreshed = createItem("old", { lastUsedAt: "2026-09-01T00:00:00.000Z" });
  queryClient.setQueryData<InfiniteData<SearchResult>>(key, {
    pages: [
      createPage(["a", "b"], 3, 0),
      createPage(
        ["old", "c"],
        3,
        2,
      ),
    ],
    pageParams: [0, 2],
  });

  applyClipsChanged({ kind: "upserted", item: refreshed });

  const data = readInfiniteData(key);
  // 第一页插头并按页大小截断，旧条目从原页移除避免双显
  assert.equal(data.pages[0].items[0].id, "old");
  assert.equal(data.pages[0].items.length, 2);
  assert.deepEqual(
    data.pages[1].items.map((item) => item.id),
    ["c"],
  );
  assert.deepEqual(
    data.pages.map((page) => page.total),
    [3, 3],
  );
});

test("applyClipsChanged upserted 在带筛选的最近列表未命中时交给 invalidate 校正", () => {
  const key = createRecentKey({ favoritedOnly: true });
  const original: InfiniteData<SearchResult> = {
    pages: [createPage(["a"], 1, 0)],
    pageParams: [0],
  };
  queryClient.setQueryData(key, original);

  applyClipsChanged({ kind: "upserted", item: createItem("plain") });

  assert.equal(queryClient.getQueryData(key), original);
  assert.equal(queryClient.getQueryState(key)?.isInvalidated, true);
});
