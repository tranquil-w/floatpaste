import { useQuery } from "@tanstack/react-query";
import { listRecentItems } from "../../bridge/commands";

export function usePickerRecentQuery(limit = 12) {
  return useQuery({
    queryKey: ["picker-recent", limit],
    queryFn: () => listRecentItems(limit),
    staleTime: 0,
    refetchOnWindowFocus: false,
  });
}
