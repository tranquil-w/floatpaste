export type SearchKeyboardAction =
  | "navigate-up"
  | "navigate-down"
  | "paste"
  | "edit-item"
  | "toggle-favorite"
  | "delete-item"
  | "close"
  | null;

export function getSearchKeyboardAction({
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
}): SearchKeyboardAction {
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
    case "Delete":
      return "delete-item";
    case "Escape":
      return "close";
    default:
      return null;
  }
}
