import { useQuery } from "@tanstack/react-query";
import { listRecentItems, searchItems } from "../../bridge/commands";
import type { SearchQuery } from "../../shared/types/clips";

const WORKBENCH_RECENT_LIMIT = 30;

export function useWorkbenchRecentQuery(enabled: boolean) {
  return useQuery({
    queryKey: ["workbench-recent", WORKBENCH_RECENT_LIMIT],
    queryFn: () => listRecentItems(WORKBENCH_RECENT_LIMIT),
    enabled,
    staleTime: 0,
  });
}

export function useWorkbenchSearchQuery(keyword: string, enabled: boolean) {
  const query: SearchQuery = {
    keyword,
    filters: {},
    offset: 0,
    limit: 50,
    sort: keyword.trim() ? "relevance_desc" : "recent_desc",
  };

  return useQuery({
    queryKey: ["workbench-search", query],
    queryFn: () => searchItems(query),
    enabled,
    staleTime: 0,
  });
}
