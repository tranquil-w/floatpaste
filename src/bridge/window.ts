import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { isTauriRuntime } from "./runtime";

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
