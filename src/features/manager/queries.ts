import { useMutation, useQuery } from "@tanstack/react-query";
import { queryClient } from "../../app/queryClient";
import { getSettings, updateSettings } from "../../bridge/commands";
import type { UserSetting } from "../../shared/types/settings";

export function useSettingsQuery() {
  return useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
  });
}

export function useUpdateSettingsMutation() {
  return useMutation({
    mutationFn: (payload: UserSetting) => updateSettings(payload),
    onSuccess: (nextValue) => {
      queryClient.setQueryData(["settings"], nextValue);
    },
  });
}
