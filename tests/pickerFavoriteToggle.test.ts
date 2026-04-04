import test from "node:test";
import assert from "node:assert/strict";
import type { ClipItemSummary } from "../src/shared/types/clips.ts";
import { toggleFavoriteSelection } from "../src/features/picker/favoriteToggle.ts";

function createItem(isFavorited: boolean): ClipItemSummary {
  return {
    id: "clip-1",
    type: "text",
    contentPreview: "hello",
    tooltipText: null,
    sourceApp: "FloatPaste",
    isFavorited,
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
  };
}

test("toggleFavoriteSelection 在成功时会刷新列表并提示已收藏", async () => {
  const calls: Array<{ id: string; value: boolean }> = [];
  const messages: string[] = [];
  let refreshed = 0;
  let errorCount = 0;

  await toggleFavoriteSelection({
    item: createItem(false),
    setItemFavorited: async (id, value) => {
      calls.push({ id, value });
    },
    refreshItems: async () => {
      refreshed += 1;
    },
    setLastMessage: (message) => {
      messages.push(message);
    },
    onError: () => {
      errorCount += 1;
    },
  });

  assert.deepEqual(calls, [{ id: "clip-1", value: true }]);
  assert.equal(refreshed, 1);
  assert.deepEqual(messages, ["已收藏"]);
  assert.equal(errorCount, 0);
});

test("toggleFavoriteSelection 在失败时会兜底提示并上报错误", async () => {
  const messages: string[] = [];
  const errors: unknown[] = [];

  const result = await toggleFavoriteSelection({
    item: createItem(true),
    setItemFavorited: async () => {
      throw new Error("boom");
    },
    refreshItems: async () => {
      throw new Error("should not refresh");
    },
    setLastMessage: (message) => {
      messages.push(message);
    },
    onError: (error) => {
      errors.push(error);
    },
  });

  assert.equal(result, false);
  assert.deepEqual(messages, ["更新收藏状态失败，请稍后重试"]);
  assert.equal(errors.length, 1);
  assert.match(String(errors[0]), /boom/);
});

test("toggleFavoriteSelection 在上一次切换未完成时会跳过重复触发", async () => {
  const calls: Array<{ id: string; value: boolean }> = [];
  let pending = false;
  let resolveFirst: (() => void) | undefined;

  const first = toggleFavoriteSelection({
    item: createItem(false),
    isPending: () => pending,
    setPending: (value) => {
      pending = value;
    },
    setItemFavorited: async (id, value) => {
      calls.push({ id, value });
      await new Promise<void>((resolve) => {
        resolveFirst = resolve;
      });
    },
    setLastMessage: () => {},
  });

  const second = await toggleFavoriteSelection({
    item: createItem(false),
    isPending: () => pending,
    setPending: (value) => {
      pending = value;
    },
    setItemFavorited: async (id, value) => {
      calls.push({ id, value });
    },
    setLastMessage: () => {},
  });

  assert.equal(second, false);
  assert.deepEqual(calls, [{ id: "clip-1", value: true }]);

  resolveFirst?.();
  await first;
  assert.equal(pending, false);
});
