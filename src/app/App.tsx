import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { queryClient } from "./queryClient";
import { SettingsShell } from "../features/settings/SettingsShell";
import { EditorShell } from "../features/editor";
import { PickerShell } from "../features/picker/PickerShell";
import { SearchShell } from "../features/search/SearchShell";
import { getSettings } from "../bridge/commands";
import { SETTINGS_CHANGED_EVENT } from "../bridge/events";
import { isTauriRuntime } from "../bridge/runtime";
import { getCurrentWindowLabel } from "../bridge/window";
import { DEFAULT_THEME_MODE, useAppliedTheme } from "../shared/theme";
import { DEFAULT_CUSTOM_THEME_COLORS } from "../shared/themeColors";

export function App() {
  const [windowLabel, setWindowLabel] = useState(() => getCurrentWindowLabel());
  const settingsQuery = useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
    staleTime: 0,
    refetchOnWindowFocus: false,
  });

  useAppliedTheme(
    settingsQuery.data?.themeMode ?? DEFAULT_THEME_MODE,
    settingsQuery.data?.customThemeColors ?? DEFAULT_CUSTOM_THEME_COLORS,
  );

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let unlistenSettings: (() => void) | undefined;
    void listen(SETTINGS_CHANGED_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }).then((cleanup) => {
      unlistenSettings = cleanup;
    });

    return () => {
      unlistenSettings?.();
    };
  }, []);

  useEffect(() => {
    const label = getCurrentWindowLabel();
    setWindowLabel(label);
    document.documentElement.classList.remove("window-picker", "window-settings", "window-search", "window-editor");
    document.body.classList.remove("theme-picker", "theme-settings", "theme-search", "theme-editor");

    if (label === "picker") {
      document.documentElement.classList.add("window-picker");
      document.body.classList.add("theme-picker");
    } else if (label === "search") {
      document.documentElement.classList.add("window-search");
      document.body.classList.add("theme-search");
    } else if (label === "editor") {
      document.documentElement.classList.add("window-editor");
      document.body.classList.add("theme-editor");
    } else {
      document.documentElement.classList.add("window-settings");
      document.body.classList.add("theme-settings");
    }
  }, []);

  if (windowLabel === "picker") {
    return <PickerShell />;
  }
  if (windowLabel === "search") {
    return <SearchShell />;
  }
  if (windowLabel === "editor") {
    return <EditorShell />;
  }
  return <SettingsShell />;
}
