import { useQuery } from "@tanstack/react-query";
import { listRecentItems, searchItems } from "../../bridge/commands";
import type { SearchQuery } from "../../shared/types/clips";

const SEARCH_RECENT_LIMIT = 30;

export function useSearchRecentQuery(enabled: boolean) {
  return useQuery({
    queryKey: ["search-recent", SEARCH_RECENT_LIMIT],
    queryFn: () => listRecentItems(SEARCH_RECENT_LIMIT),
    enabled,
    staleTime: 0,
  });
}

export function useSearchSearchQuery(keyword: string, enabled: boolean) {
  const query: SearchQuery = {
    keyword,
    filters: {},
    offset: 0,
    limit: 50,
    sort: keyword.trim() ? "relevance_desc" : "recent_desc",
  };

  return useQuery({
    queryKey: ["search-query", query],
    queryFn: () => searchItems(query),
    enabled,
    staleTime: 0,
  });
}
