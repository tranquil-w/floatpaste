import type {
  ClipItemDetail,
  ClipItemSummary,
  SearchResult,
} from "../../shared/types/clips";

export function getSearchItemFavoritedState(
  item: ClipItemSummary | null | undefined,
  detail: ClipItemDetail | null | undefined,
): boolean {
  return item?.isFavorited ?? detail?.isFavorited ?? false;
}

export function setFavoritedOnSearchResult(
  result: SearchResult | undefined,
  id: string,
  isFavorited: boolean,
  options?: {
    removeUnfavoritedItem?: boolean;
  },
): SearchResult | undefined {
  if (!result) {
    return result;
  }

  if (options?.removeUnfavoritedItem && !isFavorited) {
    const items = result.items.filter((item) => item.id !== id);
    if (items.length === result.items.length) {
      return result;
    }

    return {
      ...result,
      items,
      total: Math.max(0, result.total - 1),
    };
  }

  let updated = false;
  const items = result.items.map((item) => {
    if (item.id !== id || item.isFavorited === isFavorited) {
      return item;
    }

    updated = true;
    return {
      ...item,
      isFavorited,
    };
  });

  if (!updated) {
    return result;
  }

  return {
    ...result,
    items,
  };
}

export function setFavoritedOnDetail(
  detail: ClipItemDetail | undefined,
  id: string,
  isFavorited: boolean,
): ClipItemDetail | undefined {
  if (!detail || detail.id !== id || detail.isFavorited === isFavorited) {
    return detail;
  }

  return {
    ...detail,
    isFavorited,
  };
}
