import { useEffect, useMemo, useRef, useState } from "react";
import { emitTo, listen } from "@tauri-apps/api/event";
import { queryClient } from "../../app/queryClient";
import { hidePicker, hideSearch, openEditorFromSearch, pasteItem, setItemFavorited } from "../../bridge/commands";
import {
  CLIPS_CHANGED_EVENT,
  PICKER_CONFIRM_EVENT,
  PICKER_NAVIGATE_EVENT,
  PICKER_OPEN_EDITOR_EVENT,
  PICKER_SELECT_INDEX_EVENT,
  SEARCH_EDIT_ITEM_EVENT,
  SEARCH_INPUT_RESUME_EVENT,
  SEARCH_INPUT_SUSPEND_EVENT,
  SEARCH_NAVIGATE_EVENT,
  SEARCH_PASTE_EVENT,
  SEARCH_SESSION_END_EVENT,
  SEARCH_SESSION_START_EVENT,
} from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import type { ClipItemSummary } from "../../shared/types/clips";
import { getClipTypeLabel } from "../../shared/utils/clipDisplay";
import { getErrorMessage } from "../../shared/utils/error";
import { formatDateTime } from "../../shared/utils/time";
import { useItemDetailQuery } from "../../shared/queries/clipQueries";
import { LoadingSpinner } from "../../shared/ui/LoadingSpinner";
import { getSearchKeyboardAction } from "./keyboard";
import { useSearchRecentQuery, useSearchSearchQuery } from "./queries";
import { getNextSearchNavigationIndex } from "./state";
import { useSearchStore } from "./store";
import type { SearchSession } from "./store";

const STYLES = {
  shell:
    "flex h-screen w-screen flex-col overflow-hidden bg-pg-canvas-default text-pg-fg-default",
  searchInput:
    "w-full rounded-lg border border-pg-border-default bg-pg-canvas-inset px-3 py-2 text-sm outline-none transition-all placeholder:text-pg-fg-subtle focus:border-pg-accent-fg focus:ring-2 focus:ring-pg-accent-fg/30",
  listItem: (selected: boolean, favorited: boolean) =>
    `w-full border-b border-pg-border-muted px-4 py-3 text-left transition-colors hover:bg-pg-accent-subtle ${
      selected
        ? "bg-pg-accent-subtle border-pg-accent-fg/30"
        : favorited
          ? "border-l-[3px] border-l-pg-accent-fg"
          : ""
    }`,
  actionButton:
    "rounded-md bg-pg-accent-emphasis px-3 py-1.5 text-xs font-medium text-pg-fg-on-emphasis transition-colors hover:opacity-90",
  actionButtonSecondary:
    "rounded-md border border-pg-border-default px-3 py-1.5 text-xs font-medium text-pg-fg-default transition-colors hover:bg-pg-canvas-subtle",
};

async function refreshSearchQueries() {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["detail"] }),
    queryClient.invalidateQueries({ queryKey: ["search-recent"] }),
    queryClient.invalidateQueries({ queryKey: ["search-query"] }),
  ]);
}

export function SearchShell() {
  const {
    keyword,
    noticeMessage,
    reset,
    selectedItemId,
    session,
    setKeyword,
    setNoticeMessage,
    setSelectedItemId,
    setSession,
  } = useSearchStore();
  const searchInputRef = useRef<HTMLInputElement>(null);
  const selectedItemIdRef = useRef<string | null>(selectedItemId);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const [inputSuspended, setInputSuspended] = useState(false);
  const hasKeyword = keyword.trim().length > 0;
  const recentQuery = useSearchRecentQuery(!hasKeyword);
  const searchQuery = useSearchSearchQuery(keyword, hasKeyword);
  const items = useMemo<ClipItemSummary[]>(
    () => (hasKeyword ? (searchQuery.data?.items ?? []) : (recentQuery.data ?? [])),
    [hasKeyword, recentQuery.data, searchQuery.data?.items],
  );
  const itemsRef = useRef<ClipItemSummary[]>(items);
  const detailQuery = useItemDetailQuery(selectedItemId);

  useEffect(() => {
    itemsRef.current = items;
    selectedItemIdRef.current = selectedItemId;
  }, [items, selectedItemId]);

  // 当选中项改变时，自动滚动到视图
  useEffect(() => {
    if (!selectedItemId) {
      return;
    }
    const selectedIndex = items.findIndex((item) => item.id === selectedItemId);
    const currentItem = itemRefs.current[selectedIndex];
    if (currentItem) {
      currentItem.scrollIntoView({
        behavior: "auto",
        block: "nearest",
      });
    }
  }, [selectedItemId, items]);

  useEffect(() => {
    if (!items.length) {
      setSelectedItemId(null);
      return;
    }

    if (selectedItemId && items.some((item) => item.id === selectedItemId)) {
      return;
    }

    if (session?.initialItemId && items.some((item) => item.id === session.initialItemId)) {
      setSelectedItemId(session.initialItemId);
      return;
    }

    setSelectedItemId(items[0]?.id ?? null);
  }, [items, selectedItemId, session?.initialItemId, setSelectedItemId]);

  useEffect(() => {
    if (searchInputRef.current) {
      searchInputRef.current.focus();
    }
  }, [session?.source]);

  const navigateSelection = (direction: "up" | "down") => {
    const currentItems = itemsRef.current;
    if (!currentItems.length) {
      return;
    }

    const nextIndex = getNextSearchNavigationIndex(
      currentItems,
      selectedItemIdRef.current,
      direction,
    );

    if (nextIndex >= 0) {
      const nextId = currentItems[nextIndex]?.id ?? null;
      selectedItemIdRef.current = nextId;
      setSelectedItemId(nextId);
    }
  };

  async function forwardPickerNavigate(direction: "up" | "down") {
    try {
      await emitTo("picker", PICKER_NAVIGATE_EVENT, direction);
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(`控制速贴面板失败：${getErrorMessage(error, "请稍后重试。")}`);
    }
  }

  async function forwardPickerConfirm() {
    try {
      await emitTo("picker", PICKER_CONFIRM_EVENT);
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(`控制速贴面板失败：${getErrorMessage(error, "请稍后重试。")}`);
    }
  }

  async function forwardPickerOpenEditor() {
    try {
      await emitTo("picker", PICKER_OPEN_EDITOR_EVENT);
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(`控制速贴面板失败：${getErrorMessage(error, "请稍后重试。")}`);
    }
  }

  async function forwardPickerSelectIndex(index: number) {
    try {
      await emitTo("picker", PICKER_SELECT_INDEX_EVENT, index);
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(`控制速贴面板失败：${getErrorMessage(error, "请稍后重试。")}`);
    }
  }

  async function closePickerFromSearch() {
    try {
      await hidePicker();
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(`关闭速贴面板失败：${getErrorMessage(error, "请稍后重试。")}`);
    }
  }

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let offStart: (() => void) | undefined;
    let offClipsChanged: (() => void) | undefined;
    let offEnd: (() => void) | undefined;
    let offNavigate: (() => void) | undefined;
    let offEdit: (() => void) | undefined;
    let offPaste: (() => void) | undefined;
    let offSuspend: (() => void) | undefined;
    let offResume: (() => void) | undefined;

    void listen<{
      source: string;
      itemId?: string;
      initialKeyword?: string;
    }>(SEARCH_SESSION_START_EVENT, (event) => {
      setSession({
        source: "global" as const,
        initialItemId: event.payload.itemId,
        initialKeyword: event.payload.initialKeyword,
      } as SearchSession);
      setKeyword(event.payload.initialKeyword ?? "");
      setSelectedItemId(event.payload.itemId ?? null);
      setNoticeMessage(null);
      setInputSuspended(false);
    }).then((cleanup) => {
      offStart = cleanup;
    });

    void listen(CLIPS_CHANGED_EVENT, async () => {
      await refreshSearchQueries();
    }).then((cleanup) => {
      offClipsChanged = cleanup;
    });

    void listen(SEARCH_SESSION_END_EVENT, () => {
      setInputSuspended(false);
      reset();
    }).then((cleanup) => {
      offEnd = cleanup;
    });

    void listen<string>(SEARCH_NAVIGATE_EVENT, (event) => {
      const currentItems = itemsRef.current;
      if (!currentItems.length) {
        return;
      }

      const nextIndex = getNextSearchNavigationIndex(
        currentItems,
        selectedItemIdRef.current,
        event.payload === "up" ? "up" : "down",
      );

      if (nextIndex >= 0) {
        setSelectedItemId(currentItems[nextIndex]?.id ?? null);
      }
    }).then((cleanup) => {
      offNavigate = cleanup;
    });

    void listen(SEARCH_EDIT_ITEM_EVENT, () => {
      void handleOpenEditor();
    }).then((cleanup) => {
      offEdit = cleanup;
    });

    void listen(SEARCH_PASTE_EVENT, () => {
      void handlePasteSelected();
    }).then((cleanup) => {
      offPaste = cleanup;
    });

    void listen(SEARCH_INPUT_SUSPEND_EVENT, () => {
      setInputSuspended(true);
      searchInputRef.current?.blur();
    }).then((cleanup) => {
      offSuspend = cleanup;
    });

    void listen(SEARCH_INPUT_RESUME_EVENT, () => {
      setInputSuspended(false);
      searchInputRef.current?.focus();
    }).then((cleanup) => {
      offResume = cleanup;
    });

    return () => {
      offStart?.();
      offClipsChanged?.();
      offEnd?.();
      offNavigate?.();
      offEdit?.();
      offPaste?.();
      offSuspend?.();
      offResume?.();
    };
  }, [reset, setKeyword, setNoticeMessage, setSelectedItemId, setSession]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (inputSuspended && !event.isComposing) {
        if (event.key === "Escape") {
          event.preventDefault();
          void closePickerFromSearch();
          return;
        }

        if (event.key === "ArrowUp") {
          event.preventDefault();
          void forwardPickerNavigate("up");
          return;
        }

        if (event.key === "ArrowDown") {
          event.preventDefault();
          void forwardPickerNavigate("down");
          return;
        }

        if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
          event.preventDefault();
          void forwardPickerOpenEditor();
          return;
        }

        if (event.key === "Enter") {
          event.preventDefault();
          void forwardPickerConfirm();
          return;
        }

        if (/^[1-9]$/.test(event.key)) {
          event.preventDefault();
          void forwardPickerSelectIndex(Number(event.key) - 1);
          return;
        }
      }

      const action = getSearchKeyboardAction({
        key: event.key,
        ctrlKey: event.ctrlKey,
        metaKey: event.metaKey,
        inputSuspended,
        isComposing: event.isComposing,
      });

      if (!action) {
        return;
      }

      event.preventDefault();

      switch (action) {
        case "navigate-up":
          navigateSelection("up");
          return;
        case "navigate-down":
          navigateSelection("down");
          return;
        case "paste":
          void handlePasteSelected();
          return;
        case "edit-item":
          void handleOpenEditor();
          return;
        case "close":
          void handleClose();
          return;
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [inputSuspended, keyword, selectedItemId]);

  async function handleClose() {
    try {
      await hideSearch();
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(`关闭搜索窗口失败：${getErrorMessage(error, "请稍后重试。")}`);
    }
  }

  async function handleOpenEditor() {
    const currentItem = itemsRef.current.find((item) => item.id === selectedItemIdRef.current);
    if (!currentItem) {
      return;
    }

    if (currentItem.type !== "text") {
      setNoticeMessage("当前只支持从文本条目进入独立编辑窗口。");
      return;
    }

    try {
      await openEditorFromSearch(currentItem.id);
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(`打开编辑窗口失败：${getErrorMessage(error, "请稍后重试。")}`);
    }
  }

  async function handlePasteSelected() {
    const currentItem = itemsRef.current.find((item) => item.id === selectedItemIdRef.current);
    if (!currentItem) {
      return;
    }

    try {
      const result = await pasteItem(currentItem.id, {
        restoreClipboardAfterPaste: true,
        pasteToTarget: true,
      });
      setNoticeMessage(result.message);
    } catch (error) {
      setNoticeMessage(`执行粘贴失败：${getErrorMessage(error, "请稍后重试。")}`);
    }
  }

  async function handleToggleFavorited() {
    const id = selectedItemIdRef.current;
    if (!id) {
      return;
    }

    try {
      const favored = detailQuery.data?.isFavorited ?? false;
      await setItemFavorited(id, !favored);
      await refreshSearchQueries();
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(`更新收藏状态失败：${getErrorMessage(error, "请稍后重试。")}`);
    }
  }

  const isLoading = hasKeyword ? searchQuery.isLoading : recentQuery.isLoading;

  return (
    <div className={STYLES.shell}>
      {/* 蓝色渐变条 */}
      <div className="h-[3px] w-full shrink-0 bg-gradient-to-r from-pg-blue-5 to-pg-blue-4" />

      {/* 搜索区域 */}
      <div className="max-w-[460px] mx-auto w-full px-4 py-3">
        <input
          ref={searchInputRef}
          className={STYLES.searchInput}
          onChange={(event) => setKeyword(event.target.value)}
          placeholder="搜索剪贴板记录..."
          aria-label="搜索剪贴板记录"
          value={keyword}
        />
      </div>

      {/* 通知消息 */}
      {noticeMessage ? (
        <div className="border-b border-pg-danger-fg/20 bg-pg-danger-subtle px-4 py-2 text-sm text-pg-danger-fg">
          {noticeMessage}
        </div>
      ) : null}

      {/* 列表区域 - 单栏布局 */}
      <main className="min-h-0 flex-1 overflow-y-auto">
        {isLoading ? (
          <div className="flex h-full items-center justify-center py-12">
            <LoadingSpinner size="sm" text="加载中..." />
          </div>
        ) : items.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-1 px-4 py-12 text-center text-sm text-pg-fg-subtle">
            {hasKeyword ? (
              <>
                <span>未找到匹配记录</span>
                <span>尝试调整搜索关键词</span>
              </>
            ) : (
              <>
                <span>暂无剪贴板记录</span>
                <span>复制内容后使用 Alt+S 打开此窗口</span>
              </>
            )}
          </div>
        ) : (
          <div>
            {items.map((item, index) => {
              const isSelected = selectedItemId === item.id;
              return (
                <button
                  ref={(el) => {
                    itemRefs.current[index] = el;
                  }}
                  className={STYLES.listItem(isSelected, item.isFavorited)}
                  key={item.id}
                  onClick={() => setSelectedItemId(item.id)}
                  onDoubleClick={() => {
                    setSelectedItemId(item.id);
                    selectedItemIdRef.current = item.id;
                    void handlePasteSelected();
                  }}
                  type="button"
                >
                  <div className="flex items-center justify-between gap-2">
                    <p className="min-w-0 flex-1 line-clamp-2 text-sm text-pg-fg-default">
                      {item.contentPreview}
                    </p>
                    <div className="flex shrink-0 items-center gap-2">
                      <span className="text-xs text-pg-fg-subtle">
                        {getClipTypeLabel(item)}
                      </span>
                      {item.isFavorited && (
                        <span className="text-[12px] text-pg-favorite">★</span>
                      )}
                    </div>
                  </div>
                  <div className="mt-1 flex items-center gap-2 text-xs text-pg-fg-subtle">
                    <span>{item.sourceApp ?? "未知来源"}</span>
                    <span>{formatDateTime(item.lastUsedAt ?? item.createdAt)}</span>
                  </div>

                  {/* 选中项内联展开 */}
                  {isSelected && detailQuery.data && (
                    <div className="mt-2 border-t border-pg-accent-fg/12 pt-2">
                      {detailQuery.data.type === "text" ? (
                        <pre className="text-xs leading-relaxed text-pg-fg-muted line-clamp-5">
                          {detailQuery.data.fullText || detailQuery.data.contentPreview}
                        </pre>
                      ) : (
                        <p className="text-xs text-pg-fg-subtle">
                          当前条目不是文本类型，搜索窗口只负责搜索与定位
                        </p>
                      )}
                      <div className="mt-2 flex gap-2">
                        <button
                          className={STYLES.actionButton}
                          onMouseDown={(e) => e.preventDefault()}
                          onClick={() => void handlePasteSelected()}
                          type="button"
                        >
                          粘贴
                        </button>
                        {detailQuery.data.type === "text" && (
                          <button
                            className={STYLES.actionButtonSecondary}
                            onMouseDown={(e) => e.preventDefault()}
                            onClick={() => void handleOpenEditor()}
                            type="button"
                          >
                            编辑
                          </button>
                        )}
                        <button
                          className={STYLES.actionButtonSecondary}
                          onMouseDown={(e) => e.preventDefault()}
                          onClick={() => void handleToggleFavorited()}
                          type="button"
                        >
                          {detailQuery.data.isFavorited ? "取消收藏" : "收藏"}
                        </button>
                      </div>
                    </div>
                  )}
                </button>
              );
            })}
          </div>
        )}
      </main>

      {/* 底部快捷键提示栏 */}
      <footer className="flex shrink-0 items-center justify-center gap-3 border-t border-pg-border-subtle px-4 py-2 text-xs text-pg-fg-subtle">
        <span>
          <kbd className="rounded border border-pg-border-default bg-pg-canvas-subtle px-1.5 py-0.5 font-mono text-[10px]">Enter</kbd>
          {" "}粘贴
        </span>
        <span>
          <kbd className="rounded border border-pg-border-default bg-pg-canvas-subtle px-1.5 py-0.5 font-mono text-[10px]">Ctrl+Enter</kbd>
          {" "}编辑
        </span>
        <span>
          <kbd className="rounded border border-pg-border-default bg-pg-canvas-subtle px-1.5 py-0.5 font-mono text-[10px]">Esc</kbd>
          {" "}关闭
        </span>
      </footer>
    </div>
  );
}
