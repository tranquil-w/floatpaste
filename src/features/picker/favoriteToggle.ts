import type { ClipItemSummary } from "../../shared/types/clips";

interface ToggleFavoriteSelectionOptions {
  item: ClipItemSummary | null | undefined;
  isPending?: () => boolean;
  setPending?: (pending: boolean) => void;
  setItemFavorited: (id: string, value: boolean) => Promise<void>;
  refreshItems?: () => Promise<void>;
  setLastMessage: (message: string) => void;
  onError?: (error: unknown) => void;
}

export async function toggleFavoriteSelection({
  item,
  isPending,
  setPending,
  setItemFavorited,
  refreshItems,
  setLastMessage,
  onError,
}: ToggleFavoriteSelectionOptions): Promise<boolean> {
  if (!item || isPending?.()) {
    return false;
  }

  const nextFavorited = !item.isFavorited;
  setPending?.(true);

  try {
    await setItemFavorited(item.id, nextFavorited);
    await refreshItems?.();
    setLastMessage(nextFavorited ? "已收藏" : "已取消收藏");
    return true;
  } catch (error) {
    setLastMessage("更新收藏状态失败，请稍后重试");
    onError?.(error);
    return false;
  } finally {
    setPending?.(false);
  }
}
