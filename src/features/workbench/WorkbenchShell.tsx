import { useEffect, useMemo, useRef, useState } from "react";
import { emitTo, listen } from "@tauri-apps/api/event";
import { hidePicker, hideWorkbench, openEditorFromWorkbench, pasteItem } from "../../bridge/commands";
import {
  PICKER_CONFIRM_EVENT,
  PICKER_NAVIGATE_EVENT,
  PICKER_OPEN_EDITOR_EVENT,
  PICKER_SELECT_INDEX_EVENT,
  WORKBENCH_EDIT_ITEM_EVENT,
  WORKBENCH_INPUT_RESUME_EVENT,
  WORKBENCH_INPUT_SUSPEND_EVENT,
  WORKBENCH_NAVIGATE_EVENT,
  WORKBENCH_PASTE_EVENT,
  WORKBENCH_SESSION_END_EVENT,
  WORKBENCH_SESSION_START_EVENT,
} from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import type { ClipItemSummary } from "../../shared/types/clips";
import { getClipTypeLabel } from "../../shared/utils/clipDisplay";
import { formatDateTime } from "../../shared/utils/time";
import { useItemDetailQuery } from "../../shared/queries/clipQueries";
import { getWorkbenchKeyboardAction } from "./keyboard";
import { useWorkbenchRecentQuery, useWorkbenchSearchQuery } from "./queries";
import { getNextWorkbenchNavigationIndex } from "./state";
import { useWorkbenchStore } from "./store";

const STYLES = {
  shell:
    "flex h-screen w-screen flex-col overflow-hidden bg-[color:var(--cp-window-shell)] text-ink",
  header:
    "flex shrink-0 items-center gap-4 border-b border-[rgba(var(--cp-surface1-rgb),0.28)] bg-[rgba(var(--cp-base-rgb),0.85)] px-4 py-3 backdrop-blur-sm dark:border-[rgba(var(--cp-surface1-rgb),0.18)] dark:bg-[rgba(var(--cp-crust-rgb),0.45)]",
  searchInput:
    "flex-1 rounded-lg border border-[rgba(var(--cp-surface1-rgb),0.35)] bg-[rgba(var(--cp-base-rgb),0.75)] px-3 py-2 text-sm outline-none transition-colors placeholder:text-[color:var(--cp-text-muted)] focus:border-[rgba(var(--cp-peach-rgb),0.35)] focus:bg-[color:var(--cp-base)] dark:border-[rgba(var(--cp-surface1-rgb),0.4)] dark:bg-[rgba(var(--cp-surface0-rgb),0.4)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.6)]",
  closeButton:
    "rounded-md border border-[rgba(var(--cp-surface1-rgb),0.3)] px-3 py-2 text-sm transition-colors hover:bg-[rgba(var(--cp-surface1-rgb),0.12)] dark:border-[rgba(var(--cp-surface1-rgb),0.35)] dark:hover:bg-[rgba(var(--cp-surface1-rgb),0.18)]",
  sidebar:
    "w-[360px] shrink-0 border-r border-[color:var(--cp-border-weak)] bg-[rgba(var(--cp-mantle-rgb),0.55)] dark:bg-[rgba(var(--cp-crust-rgb),0.35)]",
  listItem: (selected: boolean) =>
    `w-full border-b border-[color:var(--cp-border-weak)] px-4 py-3 text-left transition-colors hover:bg-[rgba(var(--cp-surface1-rgb),0.12)] dark:hover:bg-[rgba(var(--cp-surface1-rgb),0.16)] ${
      selected
        ? "bg-[rgba(var(--cp-peach-rgb),0.12)] dark:bg-[rgba(var(--cp-peach-rgb),0.14)]"
        : ""
    }`,
  detailPanel: "min-w-0 flex-1 px-5 py-4 bg-[rgba(var(--cp-base-rgb),0.45)] dark:bg-transparent",
  detailCard:
    "rounded-xl border border-[rgba(var(--cp-surface1-rgb),0.3)] bg-[rgba(var(--cp-base-rgb),0.75)] p-4 dark:border-[rgba(var(--cp-surface1-rgb),0.25)] dark:bg-[rgba(var(--cp-surface0-rgb),0.2)]",
  textPreview:
    "min-h-0 flex-1 rounded-xl border border-[rgba(var(--cp-surface1-rgb),0.3)] bg-[rgba(var(--cp-base-rgb),0.75)] p-4 dark:border-[rgba(var(--cp-surface1-rgb),0.25)] dark:bg-[rgba(var(--cp-surface0-rgb),0.3)]",
  nonTextNotice:
    "rounded-xl border border-[rgba(var(--cp-surface1-rgb),0.3)] bg-[rgba(var(--cp-surface1-rgb),0.1)] p-4 text-sm leading-6 text-[color:var(--cp-text-secondary)] dark:border-[rgba(var(--cp-surface1-rgb),0.2)] dark:bg-[rgba(var(--cp-surface1-rgb),0.12)]",
};

function parseSource(_raw: string) {
  return "global" as const;
}

function getErrorMessage(error: unknown, fallback: string) {
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error;
  }

  return fallback;
}

export function WorkbenchShell() {
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
  } = useWorkbenchStore();
  const searchInputRef = useRef<HTMLInputElement>(null);
  const selectedItemIdRef = useRef<string | null>(selectedItemId);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const [inputSuspended, setInputSuspended] = useState(false);
  const hasKeyword = keyword.trim().length > 0;
  const recentQuery = useWorkbenchRecentQuery(!hasKeyword);
  const searchQuery = useWorkbenchSearchQuery(keyword, hasKeyword);
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

    const nextIndex = getNextWorkbenchNavigationIndex(
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

  async function closePickerFromWorkbench() {
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
    }>(WORKBENCH_SESSION_START_EVENT, (event) => {
      setSession({
        source: parseSource(event.payload.source),
        initialItemId: event.payload.itemId,
        initialKeyword: event.payload.initialKeyword,
      });
      setKeyword(event.payload.initialKeyword ?? "");
      setSelectedItemId(event.payload.itemId ?? null);
      setNoticeMessage(null);
      setInputSuspended(false);
    }).then((cleanup) => {
      offStart = cleanup;
    });

    void listen(WORKBENCH_SESSION_END_EVENT, () => {
      setInputSuspended(false);
      reset();
    }).then((cleanup) => {
      offEnd = cleanup;
    });

    void listen<string>(WORKBENCH_NAVIGATE_EVENT, (event) => {
      const currentItems = itemsRef.current;
      if (!currentItems.length) {
        return;
      }

      const nextIndex = getNextWorkbenchNavigationIndex(
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

    void listen(WORKBENCH_EDIT_ITEM_EVENT, () => {
      void handleOpenEditor();
    }).then((cleanup) => {
      offEdit = cleanup;
    });

    void listen(WORKBENCH_PASTE_EVENT, () => {
      void handlePasteSelected();
    }).then((cleanup) => {
      offPaste = cleanup;
    });

    void listen(WORKBENCH_INPUT_SUSPEND_EVENT, () => {
      setInputSuspended(true);
      searchInputRef.current?.blur();
    }).then((cleanup) => {
      offSuspend = cleanup;
    });

    void listen(WORKBENCH_INPUT_RESUME_EVENT, () => {
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
          void closePickerFromWorkbench();
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

      const action = getWorkbenchKeyboardAction({
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
      await hideWorkbench();
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
      await openEditorFromWorkbench(currentItem.id);
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
          <p className="text-xs font-medium uppercase tracking-[0.16em] text-[color:var(--cp-text-muted)]">
            搜索与定位
          </p>
          <h1 className="mt-1 text-base font-semibold text-[color:var(--cp-text-primary)]">
            搜索窗口
          </h1>
        </div>
        <input
          ref={searchInputRef}
          className={STYLES.searchInput}
          onChange={(event) => setKeyword(event.target.value)}
          placeholder="搜索记录..."
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
        <div className="border-b border-[rgba(var(--cp-red-rgb),0.18)] bg-[rgba(var(--cp-red-rgb),0.08)] px-4 py-2 text-sm text-[color:var(--cp-danger)]">
          {noticeMessage}
        </div>
      ) : null}

      <main className="flex min-h-0 flex-1">
        <aside className={STYLES.sidebar}>
          {isLoading ? (
            <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
              加载中...
            </div>
          ) : items.length === 0 ? (
            <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
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
                  <p className="line-clamp-2 text-sm text-[color:var(--cp-text-primary)]">
                    {item.contentPreview}
                  </p>
                  <div className="mt-2 flex items-center gap-2 text-xs text-[color:var(--cp-text-muted)]">
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
            <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
              选择一条记录查看详情，按 Enter 或双击可粘贴，Ctrl+Enter 可进入编辑器
            </div>
          ) : detailQuery.isLoading ? (
            <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
              正在加载详情...
            </div>
          ) : !detailQuery.data ? (
            <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
              未找到对应详情
            </div>
          ) : (
            <div className="flex h-full flex-col gap-4">
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <p className="text-xs font-medium uppercase tracking-[0.16em] text-[color:var(--cp-text-muted)]">
                    当前选中
                  </p>
                  <h2 className="mt-1 text-lg font-semibold text-[color:var(--cp-text-primary)]">
                    {getClipTypeLabel(detailQuery.data)}
                  </h2>
                </div>
                <button
                  className="rounded-md bg-[color:var(--cp-accent-primary)] px-4 py-2 text-sm font-semibold text-cp-base disabled:opacity-50"
                  disabled={detailQuery.data.type !== "text"}
                  onClick={() => void handleOpenEditor()}
                  type="button"
                >
                  编辑当前项
                </button>
              </div>

              <div className="grid gap-3 text-sm text-[color:var(--cp-text-secondary)] sm:grid-cols-2">
                <div className={STYLES.detailCard}>
                  <div className="text-xs uppercase tracking-[0.14em] text-[color:var(--cp-text-muted)]">
                    来源
                  </div>
                  <div className="mt-2 text-[color:var(--cp-text-primary)]">
                    {detailQuery.data.sourceApp ?? "未知来源"}
                  </div>
                </div>
                <div className={STYLES.detailCard}>
                  <div className="text-xs uppercase tracking-[0.14em] text-[color:var(--cp-text-muted)]">
                    更新时间
                  </div>
                  <div className="mt-2 text-[color:var(--cp-text-primary)]">
                    {formatDateTime(detailQuery.data.updatedAt)}
                  </div>
                </div>
              </div>

              {detailQuery.data.type === "text" ? (
                <div className={STYLES.textPreview}>
                  <pre className="h-full overflow-auto whitespace-pre-wrap break-words text-sm leading-7 text-[color:var(--cp-text-primary)]">
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
