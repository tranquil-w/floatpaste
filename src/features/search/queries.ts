import { keepPreviousData, useInfiniteQuery } from "@tanstack/react-query";
import { searchItems } from "../../bridge/commands";
import { queryKeys } from "../../shared/queries/queryKeys";
import type {
  SearchFilters,
  SearchQuery,
  SearchQuickFilter,
  SearchResult,
} from "../../shared/types/clips";

/** 最近条目与关键词搜索共用同一分页大小：触底自动加载更久远的记录 */
const SEARCH_PAGE_LIMIT = 50;

function buildFilters(filter: SearchQuickFilter, tagNames: string[]): Partial<SearchFilters> {
  // 类型与标签是独立维度，可任意组合；后端按 AND 语义合并
  const filters: Partial<SearchFilters> = {};
  if (filter === "favorite") {
    filters.favoritedOnly = true;
  } else if (filter !== "all" && filter !== "tag") {
    filters.clipType = filter;
  }
  // 兼容旧的"标签"筛选值；空数组等价于未筛选
  if (tagNames.length > 0 || filter === "tag") {
    filters.tagNames = tagNames;
  }
  return filters;
}

export function createSearchRecentQueryKey(filter: SearchQuickFilter, tagNames: string[]) {
  const query: SearchQuery = {
    keyword: "",
    filters: buildFilters(filter, tagNames),
    offset: 0,
    limit: SEARCH_PAGE_LIMIT,
    sort: "recent_desc",
  };

  return queryKeys.searchRecent(query);
}

export function createSearchSearchQueryKey(
  keyword: string,
  filter: SearchQuickFilter,
  tagNames: string[],
) {
  const query: SearchQuery = {
    keyword,
    filters: buildFilters(filter, tagNames),
    offset: 0,
    limit: SEARCH_PAGE_LIMIT,
    sort: keyword.trim() ? "relevance_desc" : "recent_desc",
  };

  return queryKeys.searchQuery(query);
}

export function useSearchRecentQuery(
  filter: SearchQuickFilter,
  tagNames: string[],
  enabled: boolean,
) {
  const queryKey = createSearchRecentQueryKey(filter, tagNames);
  const query = queryKey[1];

  return useInfiniteQuery({
    queryKey,
    // 空关键字时后端会回落到 search_recent 分支，这样最近记录与关键词搜索共用同一套筛选语义；
    // 分页让"最近条目"可以一直向下滚动加载更久远的记录，而不是只看到前一页
    queryFn: ({ pageParam }): Promise<SearchResult> => searchItems({ ...query, offset: pageParam }),
    initialPageParam: 0,
    getNextPageParam: (lastPage: SearchResult) =>
      lastPage.offset + lastPage.items.length < lastPage.total
        ? lastPage.offset + lastPage.items.length
        : undefined,
    enabled,
    staleTime: 0,
    placeholderData: keepPreviousData,
  });
}

export function useSearchSearchQuery(
  keyword: string,
  filter: SearchQuickFilter,
  tagNames: string[],
  enabled: boolean,
) {
  const queryKey = createSearchSearchQueryKey(keyword, filter, tagNames);
  const query = queryKey[1];

  return useInfiniteQuery({
    queryKey,
    queryFn: ({ pageParam }): Promise<SearchResult> => searchItems({ ...query, offset: pageParam }),
    initialPageParam: 0,
    getNextPageParam: (lastPage: SearchResult) =>
      lastPage.offset + lastPage.items.length < lastPage.total
        ? lastPage.offset + lastPage.items.length
        : undefined,
    enabled,
    staleTime: 0,
    placeholderData: keepPreviousData,
  });
}
