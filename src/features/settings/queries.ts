import { useMutation } from "@tanstack/react-query";
import { updateSettings } from "../../bridge/commands";
import type { UserSetting } from "../../shared/types/settings";

export { useSettingsQuery } from "../../shared/queries/settingsQuery";

export function useUpdateSettingsMutation() {
  return useMutation({
    mutationFn: (payload: UserSetting) => updateSettings(payload),
  });
}
