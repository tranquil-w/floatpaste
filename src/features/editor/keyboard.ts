export type EditorKeyboardAction =
  | "request-close"
  | "save"
  | "confirm-cancel"
  | "confirm-primary"
  | null;

export function getEditorKeyboardAction({
  key,
  ctrlKey,
  metaKey,
  closeConfirmOpen,
}: {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  closeConfirmOpen: boolean;
}): EditorKeyboardAction {
  if (closeConfirmOpen) {
    if (key === "Escape") {
      return "confirm-cancel";
    }

    if (key === "Enter") {
      return "confirm-primary";
    }

    return null;
  }

  if (key === "Escape") {
    return "request-close";
  }

  if ((ctrlKey || metaKey) && key.toLowerCase() === "s") {
    return "save";
  }

  return null;
}

