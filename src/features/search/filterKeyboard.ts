export type SearchFilterTriggerAction =
  | "open-next"
  | "open-prev"
  | "toggle-menu"
  | null;

export type SearchFilterOptionAction =
  | "next"
  | "prev"
  | "first"
  | "last"
  | "commit"
  | "close"
  | null;

export type SearchFilterCommitFocusTarget = "search-input";

interface SearchFilterKeyboardOptions {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
}

export function getSearchFilterTriggerAction({
  key,
  ctrlKey,
  metaKey,
}: SearchFilterKeyboardOptions): SearchFilterTriggerAction {
  if (ctrlKey || metaKey) {
    return null;
  }

  if (key === "ArrowDown") {
    return "open-next";
  }

  if (key === "ArrowUp") {
    return "open-prev";
  }

  if (key === "Enter" || key === " ") {
    return "toggle-menu";
  }

  return null;
}

export function getSearchFilterOptionAction({
  key,
  ctrlKey,
  metaKey,
}: SearchFilterKeyboardOptions): SearchFilterOptionAction {
  if (ctrlKey || metaKey) {
    return null;
  }

  switch (key) {
    case "ArrowDown":
      return "next";
    case "ArrowUp":
      return "prev";
    case "Home":
      return "first";
    case "End":
      return "last";
    case "Enter":
    case " ":
      return "commit";
    case "Escape":
      return "close";
    default:
      return null;
  }
}

export function getSearchFilterCommitFocusTarget(): SearchFilterCommitFocusTarget {
  return "search-input";
}
