import { useQuery } from "@tanstack/react-query";
import { listRecentItems } from "../../bridge/commands";
import { queryKeys } from "../../shared/queries/queryKeys";
import { useSettingsQuery } from "../../shared/queries/settingsQuery";

export const DEFAULT_PICKER_RECORD_LIMIT = 50;
/**
 * 设置数据近乎不变，靠 `settings://changed` 事件失效即可；
 * 给出可观的 staleTime，避免每次打开面板都为读取设置多付一跳 IPC。
 */
const PICKER_SETTINGS_STALE_TIME_MS = 5 * 60 * 1000;

export function normalizePickerRecordLimit(limit: number) {
  return Math.min(1000, Math.max(9, Math.trunc(limit || DEFAULT_PICKER_RECORD_LIMIT)));
}

export function usePickerSettingsQuery() {
  return useSettingsQuery({ staleTime: PICKER_SETTINGS_STALE_TIME_MS });
}

export function usePickerRecentQuery(limit = DEFAULT_PICKER_RECORD_LIMIT) {
  return useQuery({
    queryKey: queryKeys.pickerRecent(limit),
    queryFn: () => listRecentItems(limit),
    staleTime: 0,
    refetchOnWindowFocus: false,
  });
}
