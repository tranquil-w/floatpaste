import { useEffect, useState } from "react";
import { ManagerShell } from "../features/manager/ManagerShell";
import { PickerShell } from "../features/picker/PickerShell";
import { getCurrentWindowLabel } from "../bridge/window";

export function App() {
  const [windowLabel, setWindowLabel] = useState(() => getCurrentWindowLabel());

  useEffect(() => {
    const label = getCurrentWindowLabel();
    setWindowLabel(label);

    if (label === "picker") {
      document.body.classList.add("theme-picker");
    } else {
      document.body.classList.add("theme-manager");
    }
  }, []);

  return windowLabel === "picker" ? <PickerShell /> : <ManagerShell />;
}
