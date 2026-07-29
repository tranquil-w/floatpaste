import type { QueryClient } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";
import { getSettings } from "../../bridge/commands";
import type { UserSetting } from "../../shared/types/settings";

/** 设置查询的统一 queryKey，所有读写设置缓存的入口都应引用它。 */
export const settingsQueryKey = ["settings"] as const;

export type UseSettingsQueryOptions = {
  /**
   * 数据保鲜时长。临时面板（picker/search）需要即时数据，可传 0；
   * 设置窗口等长驻场景可不传，沿用 queryClient 全局默认。
   */
  staleTime?: number;
};

export function useSettingsQuery(options: UseSettingsQueryOptions = {}) {
  return useQuery<UserSetting>({
    queryKey: settingsQueryKey,
    queryFn: getSettings,
    staleTime: options.staleTime,
  });
}

/** 标记设置缓存为失效，触发相关 useSettingsQuery 重新拉取。 */
export function invalidateSettings(queryClient: QueryClient) {
  return queryClient.invalidateQueries({ queryKey: settingsQueryKey });
}
