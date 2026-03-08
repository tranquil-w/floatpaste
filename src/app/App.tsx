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
      document.body.classList.add("theme-manager");
      return;
    }

    const label = getCurrentWebviewWindow().label;
    setWindowLabel(label);
    
    if (label === "picker") {
      document.body.classList.add("theme-picker");
    } else {
      document.body.classList.add("theme-manager");
    }
  }, []);

  return windowLabel === "picker" ? <PickerShell /> : <ManagerShell />;
}
