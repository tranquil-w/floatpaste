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

export function getCurrentWindowLabel(): string {
  if (!isTauriRuntime()) {
    return "manager";
  }

  return getCurrentWebviewWindow().label;
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
