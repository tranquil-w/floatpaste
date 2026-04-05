export type WorkbenchKeyboardAction =
  | "navigate-up"
  | "navigate-down"
  | "paste"
  | "edit-item"
  | "toggle-favorite"
  | "close"
  | null;

export function getWorkbenchKeyboardAction({
  key,
  ctrlKey,
  metaKey,
  inputSuspended,
  isComposing = false,
}: {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  inputSuspended: boolean;
  isComposing?: boolean;
}): WorkbenchKeyboardAction {
  if (inputSuspended || isComposing) {
    return null;
  }

  if ((ctrlKey || metaKey) && key === "Enter") {
    return "edit-item";
  }

  if ((ctrlKey || metaKey) && key === " ") {
    return "toggle-favorite";
  }

  switch (key) {
    case "ArrowUp":
      return "navigate-up";
    case "ArrowDown":
      return "navigate-down";
    case "Enter":
      return "paste";
    case "Escape":
      return "close";
    default:
      return null;
  }
}
