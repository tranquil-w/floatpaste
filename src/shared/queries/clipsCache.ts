import type { InfiniteData } from "@tanstack/react-query";
import { queryClient } from "../../app/queryClient.ts";
import type { ClipItemSummary, ClipsChangedPayload, SearchResult } from "../types/clips";
import { invalidateClipQueries } from "./clipQueries.ts";
import { queryKeys } from "./queryKeys.ts";

function activityTime(item: ClipItemSummary): string {
  return item.lastUsedAt ?? item.createdAt;
}

/** 把最新快照合并进列表：已存在则按活跃时间是否变新决定原位替换或置顶；新条目插头并截断。 */
function mergeUpsertedSummary(
  items: ClipItemSummary[],
  incoming: ClipItemSummary,
): ClipItemSummary[] {
  const index = items.findIndex((existing) => existing.id === incoming.id);
  if (index === -1) {
    // 保持列表长度（即查询 limit 语义），避免缓存无限膨胀
    return [incoming, ...items].slice(0, Math.max(items.length, 1));
  }

  const existing = items[index];
  if (activityTime(incoming) <= activityTime(existing)) {
    const next = items.slice();
    next[index] = incoming;
    return next;
  }

  return [incoming, ...items.filter((item) => item.id !== incoming.id)];
}

function removeDeletedSummary(items: ClipItemSummary[], id: string): ClipItemSummary[] {
  return items.some((item) => item.id === id) ? items.filter((item) => item.id !== id) : items;
}

/** search-recent 缓存 queryKey 的第二段（SearchQuery）是否带筛选条件；带筛选时无法判断新条目是否命中。 */
function searchRecentHasFilters(queryKey: readonly unknown[]): boolean {
  const query = queryKey[1] as { filters?: Record<string, unknown> } | undefined;
  const active = query?.filters;
  if (!active) {
    return false;
  }
  return Boolean(
    active.favoritedOnly ||
    active.clipType ||
    active.sourceApp ||
    (Array.isArray(active.tagNames) && active.tagNames.length > 0),
  );
}

function isSearchPageData(data: unknown): data is InfiniteData<SearchResult> {
  return Boolean(data) && Array.isArray((data as InfiniteData<SearchResult>).pages);
}

/**
 * 分页缓存逐页移除已删除条目并同步每页 total；
 * 缓存内没有该条目时原样返回，避免无意义的缓存写入。
 */
function removeDeletedFromSearchPages(
  data: InfiniteData<SearchResult>,
  deletedId: string,
): InfiniteData<SearchResult> {
  const removed = data.pages.some((page) => page.items.some((item) => item.id === deletedId));
  if (!removed) {
    return data;
  }

  return {
    ...data,
    pages: data.pages.map((page) => ({
      ...page,
      items: page.items.filter((item) => item.id !== deletedId),
      total: Math.max(0, page.total - 1),
    })),
  };
}

/**
 * 把最新快照合并进分页缓存：快照合入第一页（置顶/原位替换/截断），
 * 后续页移除旧位置避免重复展示，每页 total 与是否存在保持一致。
 */
function mergeUpsertedIntoSearchPages(
  data: InfiniteData<SearchResult>,
  incoming: ClipItemSummary,
): InfiniteData<SearchResult> {
  const exists = data.pages.some((page) => page.items.some((item) => item.id === incoming.id));

  return {
    ...data,
    pages: data.pages.map((page, pageIndex) => ({
      ...page,
      items:
        pageIndex === 0
          ? mergeUpsertedSummary(page.items, incoming)
          : removeDeletedSummary(page.items, incoming.id),
      total: exists ? page.total : page.total + 1,
    })),
  };
}

/**
 * 把结构化的 `clips://changed` 载荷应用到列表缓存：
 * 单条变更走 setQueriesData 精准合并（无网络请求），仅关键词搜索与
 * 带筛选的最近列表交给后台 refetch 校正（窗口不活跃时不会真正发请求）。
 */
export function applyClipsChanged(payload: ClipsChangedPayload): void {
  if (payload.kind === "bulk-changed") {
    void invalidateClipQueries();
    return;
  }

  if (payload.kind === "upserted") {
    const incoming = payload.item;

    queryClient.setQueriesData<ClipItemSummary[]>({ queryKey: queryKeys.pickerRecents }, (items) =>
      items ? mergeUpsertedSummary(items, incoming) : items,
    );

    // search-recent 与关键词结果同为分页缓存（InfiniteData），逐页应用同一套合并
    for (const [key, data] of queryClient.getQueriesData<InfiniteData<SearchResult>>({
      queryKey: queryKeys.searchRecents,
    })) {
      if (!isSearchPageData(data)) {
        continue;
      }

      const exists = data.pages.some((page) => page.items.some((item) => item.id === incoming.id));
      if (!exists && searchRecentHasFilters(key)) {
        void queryClient.invalidateQueries({ queryKey: key });
        continue;
      }

      queryClient.setQueryData<InfiniteData<SearchResult>>(
        key,
        mergeUpsertedIntoSearchPages(data, incoming),
      );
    }

    // 关键词结果按 bm25 相关度排序，缓存内无法正确重排，交给 refetch
    void queryClient.invalidateQueries({ queryKey: queryKeys.searchQueries });
    void queryClient.invalidateQueries({ queryKey: queryKeys.clipDetail(incoming.id) });
    return;
  }

  const deletedId = payload.id;
  queryClient.setQueriesData<ClipItemSummary[]>({ queryKey: queryKeys.pickerRecents }, (items) =>
    items ? removeDeletedSummary(items, deletedId) : items,
  );
  // 删除在任何筛选/排序下都成立，可以安全地从所有缓存中移除；
  // 最近条目与关键词结果均为分页缓存（InfiniteData），逐页移除并同步每页的 total 计数
  const removeDeletedUpdater = (data: InfiniteData<SearchResult> | undefined) =>
    isSearchPageData(data) ? removeDeletedFromSearchPages(data, deletedId) : data;
  queryClient.setQueriesData<InfiniteData<SearchResult>>(
    { queryKey: queryKeys.searchRecents },
    removeDeletedUpdater,
  );
  queryClient.setQueriesData<InfiniteData<SearchResult>>(
    { queryKey: queryKeys.searchQueries },
    removeDeletedUpdater,
  );
  void queryClient.invalidateQueries({ queryKey: queryKeys.clipDetail(deletedId) });
}
