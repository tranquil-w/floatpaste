import { useQuery } from "@tanstack/react-query";
import { listFavoriteItems, listRecentItems } from "../../bridge/commands";

export function usePickerFavoritesQuery(limit = 5) {
  return useQuery({
    queryKey: ["picker-favorites", limit],
    queryFn: () => listFavoriteItems(limit),
    staleTime: 0,
    refetchOnWindowFocus: false,
  });
}

export function usePickerRecentQuery(limit = 12) {
  return useQuery({
    queryKey: ["picker-recent", limit],
    queryFn: () => listRecentItems(limit),
    staleTime: 0,
    refetchOnWindowFocus: false,
  });
}
