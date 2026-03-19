import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { isTauriRuntime } from "./runtime";

export type WindowResizeDirection =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

export function getCurrentWindowLabel(): "picker" | "workbench" | "manager" {
  if (!isTauriRuntime()) {
    return "manager";
  }

  const label = getCurrentWebviewWindow().label;
  if (label === "picker") {
    return "picker";
  }
  if (label === "workbench") {
    return "workbench";
  }
  return "manager";
}

export async function hideCurrentWindow(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await getCurrentWebviewWindow().hide();
}

export async function startCurrentWindowResize(
  direction: WindowResizeDirection,
): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await getCurrentWebviewWindow().startResizeDragging(direction);
}
