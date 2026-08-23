import type { InfiniteData } from "@tanstack/react-query";
import { queryClient } from "../../app/queryClient";
import type { ClipItemSummary, ClipsChangedPayload, SearchResult } from "../types/clips";
import { invalidateClipQueries } from "./clipQueries";
import { queryKeys } from "./queryKeys";

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

    for (const [key, data] of queryClient.getQueriesData<SearchResult>({
      queryKey: queryKeys.searchRecents,
    })) {
      if (!data) {
        continue;
      }

      const exists = data.items.some((item) => item.id === incoming.id);
      if (!exists && searchRecentHasFilters(key)) {
        void queryClient.invalidateQueries({ queryKey: key });
        continue;
      }

      queryClient.setQueryData<SearchResult>(key, {
        ...data,
        items: mergeUpsertedSummary(data.items, incoming),
        total: exists ? data.total : data.total + 1,
      });
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
  // 删除在任何筛选/排序下都成立，可以安全地从所有缓存中移除
  queryClient.setQueriesData<SearchResult>({ queryKey: queryKeys.searchRecents }, (data) =>
    data
      ? {
          ...data,
          items: removeDeletedSummary(data.items, deletedId),
          total: Math.max(0, data.total - 1),
        }
      : data,
  );
  // 关键词结果是分页缓存（InfiniteData），逐页移除并同步每页的 total 计数
  queryClient.setQueriesData<InfiniteData<SearchResult>>(
    { queryKey: queryKeys.searchQueries },
    (data) => {
      if (!data || !Array.isArray(data.pages)) {
        return data;
      }

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
    },
  );
  void queryClient.invalidateQueries({ queryKey: queryKeys.clipDetail(deletedId) });
}
