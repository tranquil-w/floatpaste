import { LogicalSize } from "@tauri-apps/api/dpi";
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

export function getCurrentWindowLabel(): "picker" | "search" | "editor" | "settings" {
  if (!isTauriRuntime()) {
    return "settings";
  }

  const label = getCurrentWebviewWindow().label;
  if (label === "picker") {
    return "picker";
  }
  if (label === "workbench") {
    return "search";
  }
  if (label === "editor") {
    return "editor";
  }
  return "settings";
}

export async function hideCurrentWindow(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await getCurrentWebviewWindow().hide();
}

export async function startCurrentWindowDragging(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await getCurrentWebviewWindow().startDragging();
}

export async function startCurrentWindowResize(
  direction: WindowResizeDirection,
): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await getCurrentWebviewWindow().startResizeDragging(direction);
}

export async function setCurrentWindowLogicalSize(
  width: number,
  height: number,
): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  await getCurrentWebviewWindow().setSize(new LogicalSize(width, height));
}

export async function setCurrentWindowLogicalSizeBounds(
  minWidth: number,
  minHeight: number,
  maxWidth: number,
  maxHeight: number,
): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }

  const window = getCurrentWebviewWindow();
  await window.setMinSize(new LogicalSize(minWidth, minHeight));
  await window.setMaxSize(new LogicalSize(maxWidth, maxHeight));
}
