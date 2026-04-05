import { useQuery } from "@tanstack/react-query";
import { searchItems } from "../../bridge/commands";
import type {
  SearchFilters,
  SearchQuery,
  SearchQuickFilter,
  SearchResult,
} from "../../shared/types/clips";

const SEARCH_RECENT_LIMIT = 30;

function buildFilters(filter: SearchQuickFilter): Partial<SearchFilters> {
  if (filter === "favorite") {
    return { favoritedOnly: true } as const;
  }

  if (filter === "all") {
    return {};
  }

  return { clipType: filter } as const;
}

export function createSearchRecentQueryKey(filter: SearchQuickFilter) {
  const query: SearchQuery = {
    keyword: "",
    filters: buildFilters(filter),
    offset: 0,
    limit: SEARCH_RECENT_LIMIT,
    sort: "recent_desc",
  };

  return ["search-recent", query] as const;
}

export function createSearchSearchQueryKey(
  keyword: string,
  filter: SearchQuickFilter,
) {
  const query: SearchQuery = {
    keyword,
    filters: buildFilters(filter),
    offset: 0,
    limit: 50,
    sort: keyword.trim() ? "relevance_desc" : "recent_desc",
  };

  return ["search-query", query] as const;
}

export function useSearchRecentQuery(filter: SearchQuickFilter, enabled: boolean) {
  const queryKey = createSearchRecentQueryKey(filter);
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
  enabled: boolean,
) {
  const queryKey = createSearchSearchQueryKey(keyword, filter);
  const query = queryKey[1];

  return useQuery({
    queryKey,
    queryFn: () => searchItems(query),
    enabled,
    staleTime: 0,
  });
}
