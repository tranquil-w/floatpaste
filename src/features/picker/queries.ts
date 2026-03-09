import { useQuery } from "@tanstack/react-query";
import { getSettings, listRecentItems } from "../../bridge/commands";

export const DEFAULT_PICKER_RECORD_LIMIT = 50;

export function normalizePickerRecordLimit(limit: number) {
  return Math.min(1000, Math.max(9, Math.trunc(limit || DEFAULT_PICKER_RECORD_LIMIT)));
}

export function usePickerSettingsQuery() {
  return useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
    staleTime: 0,
    refetchOnWindowFocus: false,
  });
}

export function usePickerRecentQuery(limit = DEFAULT_PICKER_RECORD_LIMIT, enabled = true) {
  return useQuery({
    queryKey: ["picker-recent", limit],
    queryFn: () => listRecentItems(limit),
    enabled,
    staleTime: 0,
    refetchOnWindowFocus: false,
  });
}
