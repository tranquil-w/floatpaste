import type { ClipItemDetail, ClipItemSummary } from "../../shared/types/clips";

export function getCachedTextStateForSelection(detail: ClipItemDetail | null | undefined) {
  if (!detail || detail.type !== "text") {
    return null;
  }

  const nextText = detail.fullText ?? "";
  return {
    draftText: nextText,
    savedText: nextText,
  };
}

export function getNextWorkbenchNavigationIndex(
  items: ClipItemSummary[],
  selectedItemId: string | null,
  direction: "up" | "down",
) {
  if (!items.length) {
    return -1;
  }

  const currentIndex = Math.max(
    0,
    items.findIndex((item) => item.id === selectedItemId),
  );

  return direction === "up"
    ? (currentIndex - 1 + items.length) % items.length
    : (currentIndex + 1) % items.length;
}
