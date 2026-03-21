import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { queryClient } from "./queryClient";
import { ManagerShell } from "../features/manager/ManagerShell";
import { EditorShell } from "../features/editor";
import { PickerShell } from "../features/picker/PickerShell";
import { WorkbenchShell } from "../features/workbench/WorkbenchShell";
import { getSettings } from "../bridge/commands";
import { SETTINGS_CHANGED_EVENT } from "../bridge/events";
import { isTauriRuntime } from "../bridge/runtime";
import { getCurrentWindowLabel } from "../bridge/window";
import { DEFAULT_THEME_MODE, useAppliedTheme } from "../shared/theme";

export function App() {
  const [windowLabel, setWindowLabel] = useState(() => getCurrentWindowLabel());
  const settingsQuery = useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
    staleTime: 0,
    refetchOnWindowFocus: false,
  });

  useAppliedTheme(settingsQuery.data?.themeMode ?? DEFAULT_THEME_MODE);

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
    document.documentElement.classList.remove("window-picker", "window-manager", "window-workbench", "window-editor");
    document.body.classList.remove("theme-picker", "theme-manager", "theme-workbench", "theme-editor");

    if (label === "picker") {
      document.documentElement.classList.add("window-picker");
      document.body.classList.add("theme-picker");
    } else if (label === "workbench") {
      document.documentElement.classList.add("window-workbench");
      document.body.classList.add("theme-workbench");
    } else if (label === "editor") {
      document.documentElement.classList.add("window-editor");
      document.body.classList.add("theme-editor");
    } else {
      document.documentElement.classList.add("window-manager");
      document.body.classList.add("theme-manager");
    }
  }, []);

  if (windowLabel === "picker") {
    return <PickerShell />;
  }
  if (windowLabel === "workbench") {
    return <WorkbenchShell />;
  }
  if (windowLabel === "editor") {
    return <EditorShell />;
  }
  return <ManagerShell />;
}
