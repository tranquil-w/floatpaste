import { useEffect, useMemo, useRef, useState } from "react";
import { emitTo, listen } from "@tauri-apps/api/event";
import { hidePicker, hideSearch, openEditorFromSearch, pasteItem } from "../../bridge/commands";
import {
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
import { getSearchKeyboardAction } from "./keyboard";
import { useSearchRecentQuery, useSearchSearchQuery } from "./queries";
import { getNextSearchNavigationIndex } from "./state";
import { useSearchStore } from "./store";
import type { SearchSession } from "./store";

const STYLES = {
  shell:
    "flex h-screen w-screen flex-col overflow-hidden bg-pg-canvas-default text-pg-fg-default",
  header:
    "flex shrink-0 items-center gap-4 border-b border-pg-border-subtle bg-pg-canvas-subtle px-4 py-3",
  searchInput:
    "flex-1 rounded-lg border border-pg-border-default bg-pg-canvas-inset px-3 py-2 text-sm outline-none transition-colors placeholder:text-pg-fg-subtle focus:border-pg-accent-fg focus:ring-1 focus:ring-pg-accent-fg",
  closeButton:
    "rounded-md border border-pg-border-default px-3 py-2 text-sm transition-colors hover:bg-pg-canvas-subtle",
  sidebar:
    "w-[360px] shrink-0 border-r border-pg-border-muted bg-pg-canvas-subtle",
  listItem: (selected: boolean) =>
    `w-full border-b border-pg-border-muted px-4 py-3 text-left transition-colors hover:bg-pg-accent-subtle ${
      selected
        ? "bg-pg-accent-subtle"
        : ""
    }`,
  detailPanel: "min-w-0 flex-1 px-5 py-4 bg-pg-canvas-default",
  detailCard:
    "rounded-lg border border-pg-border-default bg-pg-canvas-subtle p-4",
  textPreview:
    "min-h-0 flex-1 rounded-lg border border-pg-border-default bg-pg-canvas-subtle p-4",
  nonTextNotice:
    "rounded-lg border border-pg-border-default bg-pg-canvas-subtle p-4 text-sm leading-6 text-pg-fg-muted",
};

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

  const isLoading = hasKeyword ? searchQuery.isLoading : recentQuery.isLoading;

  return (
    <div className={STYLES.shell}>
      <header className={STYLES.header}>
        <div className="min-w-0" data-tauri-drag-region>
          <p className="text-xs font-medium uppercase tracking-[0.16em] text-pg-fg-subtle">
            搜索与定位
          </p>
          <h1 className="mt-1 text-base font-semibold text-pg-fg-default">
            搜索窗口
          </h1>
        </div>
        <input
          ref={searchInputRef}
          className={STYLES.searchInput}
          onChange={(event) => setKeyword(event.target.value)}
          placeholder="搜索记录..."
          aria-label="搜索记录"
          value={keyword}
        />
        <button
          className={STYLES.closeButton}
          onClick={() => void handleClose()}
          type="button"
        >
          关闭
        </button>
      </header>

      {noticeMessage ? (
        <div className="border-b border-pg-danger-fg/20 bg-pg-danger-subtle px-4 py-2 text-sm text-pg-danger-fg">
          {noticeMessage}
        </div>
      ) : null}

      <main className="flex min-h-0 flex-1">
        <aside className={STYLES.sidebar}>
          {isLoading ? (
            <div className="flex h-full items-center justify-center text-sm text-pg-fg-subtle">
              加载中...
            </div>
          ) : items.length === 0 ? (
            <div className="flex h-full items-center justify-center text-sm text-pg-fg-subtle">
              {hasKeyword ? "未找到匹配记录" : "暂无记录"}
            </div>
          ) : (
            <div className="h-full overflow-y-auto">
              {items.map((item, index) => (
                <button
                  ref={(el) => {
                    itemRefs.current[index] = el;
                  }}
                  className={STYLES.listItem(selectedItemId === item.id)}
                  key={item.id}
                  onClick={() => setSelectedItemId(item.id)}
                  onDoubleClick={() => {
                    setSelectedItemId(item.id);
                    selectedItemIdRef.current = item.id;
                    void handlePasteSelected();
                  }}
                  type="button"
                >
                  <p className="line-clamp-2 text-sm text-pg-fg-default">
                    {item.contentPreview}
                  </p>
                  <div className="mt-2 flex items-center gap-2 text-xs text-pg-fg-subtle">
                    <span>{getClipTypeLabel(item)}</span>
                    <span>{item.sourceApp ?? "未知来源"}</span>
                    <span>{formatDateTime(item.lastUsedAt ?? item.createdAt)}</span>
                  </div>
                </button>
              ))}
            </div>
          )}
        </aside>

        <section className={STYLES.detailPanel}>
          {!selectedItemId ? (
            <div className="flex h-full items-center justify-center text-sm text-pg-fg-subtle">
              选择一条记录查看详情，按 Enter 或双击可粘贴，Ctrl+Enter 可进入编辑器
            </div>
          ) : detailQuery.isLoading ? (
            <div className="flex h-full items-center justify-center text-sm text-pg-fg-subtle">
              正在加载详情...
            </div>
          ) : !detailQuery.data ? (
            <div className="flex h-full items-center justify-center text-sm text-pg-fg-subtle">
              未找到对应详情
            </div>
          ) : (
            <div className="flex h-full flex-col gap-4">
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <p className="text-xs font-medium uppercase tracking-[0.16em] text-pg-fg-subtle">
                    当前选中
                  </p>
                  <h2 className="mt-1 text-lg font-semibold text-pg-fg-default">
                    {getClipTypeLabel(detailQuery.data)}
                  </h2>
                </div>
                <button
                  className="rounded-md bg-pg-accent-emphasis px-4 py-2 text-sm font-semibold text-pg-fg-on-emphasis disabled:opacity-50"
                  disabled={detailQuery.data.type !== "text"}
                  onClick={() => void handleOpenEditor()}
                  type="button"
                >
                  编辑当前项
                </button>
              </div>

              <div className="grid gap-3 text-sm text-pg-fg-muted sm:grid-cols-2">
                <div className={STYLES.detailCard}>
                  <div className="text-xs uppercase tracking-[0.14em] text-pg-fg-subtle">
                    来源
                  </div>
                  <div className="mt-2 text-pg-fg-default">
                    {detailQuery.data.sourceApp ?? "未知来源"}
                  </div>
                </div>
                <div className={STYLES.detailCard}>
                  <div className="text-xs uppercase tracking-[0.14em] text-pg-fg-subtle">
                    更新时间
                  </div>
                  <div className="mt-2 text-pg-fg-default">
                    {formatDateTime(detailQuery.data.updatedAt)}
                  </div>
                </div>
              </div>

              {detailQuery.data.type === "text" ? (
                <div className={STYLES.textPreview}>
                  <pre className="h-full overflow-auto whitespace-pre-wrap break-words text-sm leading-7 text-pg-fg-default">
                    {detailQuery.data.fullText || detailQuery.data.contentPreview}
                  </pre>
                </div>
              ) : (
                <div className={STYLES.nonTextNotice}>
                  当前条目不是文本类型，搜索窗口只负责搜索与定位；如需编辑，请选择文本条目后再进入独立编辑窗口。
                </div>
              )}
            </div>
          )}
        </section>
      </main>
    </div>
  );
}
