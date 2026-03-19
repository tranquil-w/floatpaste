import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ManagerShell } from "../features/manager/ManagerShell";
import { PickerShell } from "../features/picker/PickerShell";
import { WorkbenchShell } from "../features/workbench/WorkbenchShell";
import { getSettings } from "../bridge/commands";
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
    const label = getCurrentWindowLabel();
    setWindowLabel(label);
    document.documentElement.classList.remove("window-picker", "window-manager", "window-workbench");
    document.body.classList.remove("theme-picker", "theme-manager", "theme-workbench");

    if (label === "picker") {
      document.documentElement.classList.add("window-picker");
      document.body.classList.add("theme-picker");
    } else if (label === "workbench") {
      document.documentElement.classList.add("window-workbench");
      document.body.classList.add("theme-workbench");
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
  return <ManagerShell />;
}
