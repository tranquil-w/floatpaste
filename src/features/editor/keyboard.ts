export type EditorKeyboardAction =
  | "request-close"
  | "save"
  | "confirm-cancel"
  | "trap-confirm-focus"
  | null;

const DIALOG_FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "[href]",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(", ");

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

    if (key === "Tab") {
      return "trap-confirm-focus";
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

export function moveFocusInDialog({
  activeElement,
  container,
  shiftKey,
}: {
  activeElement: Element | null;
  container: HTMLElement | null;
  shiftKey: boolean;
}) {
  if (!container) {
    return false;
  }

  const focusableElements = Array.from(
    container.querySelectorAll<HTMLElement>(DIALOG_FOCUSABLE_SELECTOR),
  ).filter((element) => element.tabIndex !== -1);

  if (!focusableElements.length) {
    return false;
  }

  const currentIndex =
    activeElement instanceof HTMLElement ? focusableElements.indexOf(activeElement) : -1;
  const nextIndex =
    currentIndex === -1
      ? shiftKey
        ? focusableElements.length - 1
        : 0
      : (currentIndex + (shiftKey ? -1 : 1) + focusableElements.length) % focusableElements.length;

  focusableElements[nextIndex]?.focus();
  return true;
}
