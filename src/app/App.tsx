import { useEffect, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ManagerShell } from "../features/manager/ManagerShell";
import { PickerShell } from "../features/picker/PickerShell";
import { isTauriRuntime } from "../bridge/runtime";

export function App() {
  const [windowLabel, setWindowLabel] = useState(() => {
    if (!isTauriRuntime()) {
      return "manager";
    }

    return getCurrentWebviewWindow().label;
  });

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    setWindowLabel(getCurrentWebviewWindow().label);
  }, []);

  return windowLabel === "picker" ? <PickerShell /> : <ManagerShell />;
}
