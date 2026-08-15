import { useQuery } from "@tanstack/react-query";
import { listTags, searchItems } from "../../bridge/commands";
import type {
  SearchFilters,
  SearchQuery,
  SearchQuickFilter,
  SearchResult,
} from "../../shared/types/clips";

const SEARCH_RECENT_LIMIT = 30;

function buildFilters(filter: SearchQuickFilter, tagNames: string[]): Partial<SearchFilters> {
  if (filter === "favorite") {
    return { favoritedOnly: true } as const;
  }

  if (filter === "all") {
    return {};
  }

  if (filter === "tag") {
    // 空数组等价于未筛选；后端按 AND 语义组合多标签
    return { tagNames };
  }

  return { clipType: filter } as const;
}

export function createSearchRecentQueryKey(filter: SearchQuickFilter, tagNames: string[]) {
  const query: SearchQuery = {
    keyword: "",
    filters: buildFilters(filter, tagNames),
    offset: 0,
    limit: SEARCH_RECENT_LIMIT,
    sort: "recent_desc",
  };

  return ["search-recent", query] as const;
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
    // 关键词搜索一次取回更多结果，避免匹配超过 50 条时被截断导致"漏匹配"
    limit: 200,
    sort: keyword.trim() ? "relevance_desc" : "recent_desc",
  };

  return ["search-query", query] as const;
}

export function useSearchRecentQuery(
  filter: SearchQuickFilter,
  tagNames: string[],
  enabled: boolean,
) {
  const queryKey = createSearchRecentQueryKey(filter, tagNames);
  const query = queryKey[1];

  return useQuery({
    queryKey,
    // 空关键字时后端会回落到 search_recent 分支，这样最近记录与关键词搜索共用同一套筛选语义。
    queryFn: (): Promise<SearchResult> => searchItems(query),
    enabled,
    staleTime: 0,
  });
}

export function useSearchSearchQuery(
  keyword: string,
  filter: SearchQuickFilter,
  tagNames: string[],
  enabled: boolean,
) {
  const queryKey = createSearchSearchQueryKey(keyword, filter, tagNames);

  return useQuery({
    queryKey,
    queryFn: () => searchItems(queryKey[1]),
    enabled,
    staleTime: 0,
  });
}

export function useTagsQuery(enabled: boolean) {
  return useQuery({
    queryKey: ["tags"],
    queryFn: () => listTags(),
    enabled,
    staleTime: 0,
  });
}
