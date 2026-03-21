import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useWorkbenchStore } from "./store";
import {
  WORKBENCH_NAVIGATE_EVENT,
  WORKBENCH_SESSION_END_EVENT,
  WORKBENCH_SESSION_START_EVENT,
} from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import { useWorkbenchRecentQuery, useWorkbenchSearchQuery } from "./queries";
import {
  useDeleteItemMutation,
  useItemDetailQuery,
  usePasteMutation,
  useSetFavoritedMutation,
  useUpdateTextMutation,
} from "../manager/queries";
import { hideWorkbench } from "../../bridge/commands";
import type { ClipItemDetail, ClipItemSummary } from "../../shared/types/clips";
import { getClipTypeLabel } from "../../shared/utils/clipDisplay";
import { formatDateTime } from "../../shared/utils/time";
import { queryClient } from "../../app/queryClient";
import {
  getCachedTextStateForSelection,
  getNextWorkbenchNavigationIndex,
} from "./state";

type WorkbenchSource = "picker_edit" | "picker_search" | "global";

function parseSource(raw: string): WorkbenchSource {
  if (raw === "picker_edit" || raw === "picker_search" || raw === "global") {
    return raw;
  }

  return "global";
}

function getErrorMessage(error: unknown, fallback: string): string {
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
    noticeMessage,
    setKeyword,
    setMode,
    setNoticeMessage,
    setSelectedItemId,
    setSession,
  } = useWorkbenchStore();
  const [pendingSelectId, setPendingSelectId] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let offStart: (() => void) | undefined;
    let offEnd: (() => void) | undefined;

    void listen<{
      source: string;
      itemId?: string;
      initialKeyword?: string;
    }>(WORKBENCH_SESSION_START_EVENT, (event) => {
      const { source, itemId, initialKeyword } = event.payload;
      setSession({
        source: parseSource(source),
        initialItemId: itemId,
        initialKeyword,
      });
      if (itemId) {
        setMode("edit");
        setSelectedItemId(itemId);
      } else {
        setMode("search");
        setSelectedItemId(null);
      }
      setKeyword(initialKeyword ?? "");
      setNoticeMessage(null);
    })
      .then((cleanup) => {
        offStart = cleanup;
      })
      .catch((error: unknown) => {
        setNoticeMessage(
          `工作窗初始化失败：${getErrorMessage(error, "无法监听启动事件，请重新打开工作窗。")}`,
        );
      });

    void listen(WORKBENCH_SESSION_END_EVENT, () => {
      setSession(null);
      setMode("search");
      setSelectedItemId(null);
      setKeyword("");
      setNoticeMessage(null);
    })
      .then((cleanup) => {
        offEnd = cleanup;
      })
      .catch((error: unknown) => {
        setNoticeMessage(
          `工作窗清理失败：${getErrorMessage(error, "无法监听关闭事件，请重新打开工作窗。")}`,
        );
      });

    return () => {
      offStart?.();
      offEnd?.();
    };
  }, [setKeyword, setMode, setNoticeMessage, setSelectedItemId, setSession]);

  return (
    <div className="h-screen w-screen overflow-hidden bg-[color:var(--cp-window-shell)] text-ink">
      <div className="flex h-full flex-col">
        <header className="shrink-0 border-b border-[color:var(--cp-border-weak)] px-4 py-3">
          <TopBar />
        </header>
        {noticeMessage ? <NoticeBanner message={noticeMessage} /> : null}
        <main className="flex min-h-0 flex-1">
          <aside className="w-80 shrink-0 border-r border-[color:var(--cp-border-weak)]">
            <ResultList pendingSelectId={pendingSelectId} onPendingSelect={setPendingSelectId} />
          </aside>
          <section className="min-w-0 flex-1">
            <EditPanel />
          </section>
        </main>
      </div>
      <ConfirmDialog pendingSelectId={pendingSelectId} onPendingSelectChange={setPendingSelectId} />
    </div>
  );
}

function NoticeBanner({ message }: { message: string }) {
  return (
    <div className="border-b border-[rgba(var(--cp-red-rgb),0.18)] bg-[rgba(var(--cp-red-rgb),0.08)] px-4 py-2.5 text-sm text-[color:var(--cp-danger)]">
      {message}
    </div>
  );
}

function TopBar() {
  const { keyword, session, setKeyword, setNoticeMessage } = useWorkbenchStore();
  const searchInputRef = useRef<HTMLInputElement>(null);

  // 当从 picker 搜索或全局快捷键进入时，自动聚焦搜索框
  useEffect(() => {
    if (session && (session.source === "picker_search" || session.source === "global") && searchInputRef.current) {
      searchInputRef.current.focus();
    }
  }, [session]);

  const handleClose = async () => {
    try {
      await hideWorkbench();
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(
        `关闭工作窗失败：${getErrorMessage(error, "请稍后重试。")}`,
      );
    }
  };

  return (
    <div className="flex items-center gap-4">
      <span className="text-xs text-[color:var(--cp-text-muted)]">
        {session?.source === "picker_edit" && "从 Picker 编辑"}
        {session?.source === "picker_search" && "从 Picker 搜索"}
        {session?.source === "global" && "全局搜索"}
      </span>
      <input
        ref={searchInputRef}
        className="flex-1 rounded-md border border-[color:var(--cp-border-weak)] bg-[color:var(--cp-control-surface)] px-3 py-2 text-sm outline-none focus:border-[rgba(var(--cp-peach-rgb),0.35)]"
        placeholder="搜索记录..."
        value={keyword}
        onChange={(event) => setKeyword(event.target.value)}
      />
      <button
        className="rounded-md border border-[color:var(--cp-border-weak)] px-3 py-1.5 text-sm hover:bg-[rgba(var(--cp-surface1-rgb),0.1)]"
        onClick={() => void handleClose()}
        type="button"
      >
        关闭
      </button>
    </div>
  );
}

function ResultList({
  pendingSelectId,
  onPendingSelect,
}: {
  pendingSelectId: string | null;
  onPendingSelect: (id: string) => void;
}) {
  const {
    draftText,
    isDirty,
    keyword,
    selectedItemId,
    setDraftText,
    setIsDirty,
    setMode,
    setNoticeMessage,
    setSavedText,
    setSelectedItemId,
  } = useWorkbenchStore();

  const hasKeyword = keyword.trim().length > 0;
  const recent = useWorkbenchRecentQuery(!hasKeyword);
  const search = useWorkbenchSearchQuery(keyword, hasKeyword);
  const items: ClipItemSummary[] = hasKeyword ? (search.data?.items ?? []) : (recent.data ?? []);
  const isLoading = hasKeyword ? search.isLoading : recent.isLoading;

  // 使用 ref 来存储最新的状态值，避免闭包陷阱
  const itemsRef = useRef<ClipItemSummary[]>(items);
  const selectedItemIdRef = useRef<string | null>(selectedItemId);
  const isDirtyRef = useRef<boolean>(isDirty);

  // 同步 ref 值
  useEffect(() => {
    itemsRef.current = items;
    selectedItemIdRef.current = selectedItemId;
    isDirtyRef.current = isDirty;
  }, [items, selectedItemId, isDirty]);

  const handleSelectItem = useCallback((item: ClipItemSummary) => {
    if (isDirtyRef.current && selectedItemIdRef.current) {
      onPendingSelect(item.id);
      return;
    }

    selectedItemIdRef.current = item.id;
    setSelectedItemId(item.id);
    setMode("edit");
    setIsDirty(false);
    setNoticeMessage(null);

    const cachedTextState = getCachedTextStateForSelection(
      queryClient.getQueryData<ClipItemDetail>(["detail", item.id]),
    );

    if (cachedTextState) {
      setDraftText(cachedTextState.draftText);
      setSavedText(cachedTextState.savedText);
      return;
    }

    setDraftText("");
    setSavedText("");
  }, [onPendingSelect, setSelectedItemId, setMode, setDraftText, setIsDirty, setSavedText, setNoticeMessage]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let offNavigate: (() => void) | undefined;

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
      handleSelectItem(currentItems[nextIndex]);
    })
      .then((cleanup) => {
        offNavigate = cleanup;
      })
      .catch((error: unknown) => {
        setNoticeMessage(
          `键盘导航不可用：${getErrorMessage(error, "请重新打开工作窗。")}`,
        );
      });

    return () => {
      offNavigate?.();
    };
  }, [handleSelectItem, setNoticeMessage]);

  void pendingSelectId;

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
        加载中...
      </div>
    );
  }

  if (items.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
        {hasKeyword ? "未找到匹配记录" : "暂无记录"}
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      {items.map((item) => (
        <button
          key={item.id}
          className={`w-full border-b border-[color:var(--cp-border-weak)] px-4 py-3 text-left transition-colors hover:bg-[rgba(var(--cp-surface1-rgb),0.1)] ${
            selectedItemId === item.id ? "bg-[rgba(var(--cp-peach-rgb),0.08)]" : ""
          }`}
          onClick={() => handleSelectItem(item)}
          type="button"
        >
          <p className="line-clamp-2 text-sm text-[color:var(--cp-text-primary)]">
            {item.contentPreview}
          </p>
          <div className="mt-1 flex items-center gap-2 text-xs text-[color:var(--cp-text-muted)]">
            <span>{getClipTypeLabel(item)}</span>
            <span>{formatDateTime(item.lastUsedAt ?? item.createdAt)}</span>
            {item.isFavorited ? (
              <span className="text-[color:var(--cp-favorite)]">★</span>
            ) : null}
          </div>
        </button>
      ))}
    </div>
  );
}

function EditPanel() {
  const {
    draftText,
    isDirty,
    savedText,
    selectedItemId,
    session,
    setDraftText,
    setIsDirty,
    setMode,
    setNoticeMessage,
    setSavedText,
    setSelectedItemId,
  } = useWorkbenchStore();

  const detail = useItemDetailQuery(selectedItemId);
  const updateTextMutation = useUpdateTextMutation();
  const favoritedMutation = useSetFavoritedMutation();
  const deleteMutation = useDeleteItemMutation();
  const pasteMutation = usePasteMutation();
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // 当从 picker_edit 进入编辑模式时，自动聚焦编辑框
  useEffect(() => {
    if (
      session?.source === "picker_edit" &&
      selectedItemId &&
      detail.data?.type === "text" &&
      textareaRef.current
    ) {
      textareaRef.current.focus();
    }
  }, [session?.source, selectedItemId, detail.data?.type]);

  useEffect(() => {
    if (detail.data && detail.data.type === "text") {
      setDraftText(detail.data.fullText ?? "");
      setSavedText(detail.data.fullText ?? "");
    }
  }, [detail.data, setDraftText, setSavedText]);

  useEffect(() => {
    setIsDirty(draftText !== savedText);
  }, [draftText, savedText, setIsDirty]);

  const handleSave = async () => {
    if (!selectedItemId) {
      return;
    }

    try {
      await updateTextMutation.mutateAsync({ id: selectedItemId, text: draftText });
      setSavedText(draftText);
      setIsDirty(false);
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(
        `保存失败：${getErrorMessage(error, "当前修改未保存，请稍后重试。")}`,
      );
    }
  };

  const handleToggleFavorite = async () => {
    if (!selectedItemId || !detail.data) {
      return;
    }

    try {
      await favoritedMutation.mutateAsync({
        id: selectedItemId,
        value: !detail.data.isFavorited,
      });
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(
        `收藏操作失败：${getErrorMessage(error, "请稍后重试。")}`,
      );
    }
  };

  const handleDelete = async () => {
    if (!selectedItemId) {
      return;
    }

    try {
      await deleteMutation.mutateAsync(selectedItemId);
      setDraftText("");
      setSelectedItemId(null);
      setMode("search");
      setIsDirty(false);
      setSavedText("");
      setNoticeMessage(null);
    } catch (error) {
      setNoticeMessage(
        `删除失败：${getErrorMessage(error, "记录未删除，请稍后重试。")}`,
      );
    }
  };

  const handlePaste = async () => {
    if (!selectedItemId) {
      return;
    }

    if (isDirty) {
      setNoticeMessage("回贴前请先保存修改，或放弃当前改动后再继续。");
      return;
    }

    try {
      const result = await pasteMutation.mutateAsync({
        id: selectedItemId,
        option: { restoreClipboardAfterPaste: true, pasteToTarget: true },
      });

      if (!result.success) {
        setNoticeMessage(result.message || "回贴失败，请稍后重试。");
        return;
      }

      setNoticeMessage(null);
      await hideWorkbench();
    } catch (error) {
      setNoticeMessage(
        `回贴失败：${getErrorMessage(error, "未能完成回贴，请稍后重试。")}`,
      );
    }
  };

  if (!selectedItemId) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
        选择一条记录进行编辑
      </div>
    );
  }

  if (detail.isLoading) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
        加载中...
      </div>
    );
  }

  if (!detail.data) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
        记录不存在
      </div>
    );
  }

  const isTextItem = detail.data.type === "text";

  return (
    <div className="flex h-full flex-col">
      <div className="min-h-0 flex-1 p-4">
        {isTextItem ? (
          <textarea
            ref={textareaRef}
            className="h-full w-full resize-none rounded-md border border-[color:var(--cp-border-weak)] bg-[color:var(--cp-control-surface)] p-4 text-sm outline-none focus:border-[rgba(var(--cp-peach-rgb),0.35)]"
            value={draftText}
            onChange={(event) => setDraftText(event.target.value)}
            placeholder="输入内容..."
          />
        ) : (
          <MetaInfoPanel detail={detail.data} />
        )}
      </div>

      <div className="shrink-0 border-t border-[color:var(--cp-border-weak)] px-4 py-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <button
              className="rounded-md bg-[color:var(--cp-accent-primary)] px-4 py-2 text-sm font-semibold text-cp-base disabled:opacity-50"
              disabled={!isDirty || updateTextMutation.isPending}
              onClick={() => void handleSave()}
              type="button"
            >
              保存
            </button>
            <button
              className="rounded-md border border-[color:var(--cp-border-weak)] px-4 py-2 text-sm hover:bg-[rgba(var(--cp-surface1-rgb),0.1)]"
              onClick={() => void handleToggleFavorite()}
              type="button"
            >
              {detail.data.isFavorited ? "取消收藏" : "收藏"}
            </button>
            <button
              className="rounded-md border border-[color:var(--cp-border-weak)] px-4 py-2 text-sm text-red-500 hover:bg-red-50"
              onClick={() => void handleDelete()}
              type="button"
            >
              删除
            </button>
          </div>
          <button
            className="rounded-md bg-[color:var(--cp-accent-primary-strong)] px-6 py-2 text-sm font-semibold text-cp-base"
            onClick={() => void handlePaste()}
            type="button"
          >
            回贴
          </button>
        </div>
      </div>
    </div>
  );
}

function MetaInfoPanel({ detail }: { detail: ClipItemDetail }) {
  return (
    <div className="space-y-4 rounded-md border border-[color:var(--cp-border-weak)] p-4">
      <h3 className="font-semibold text-[color:var(--cp-text-primary)]">条目信息</h3>
      <dl className="space-y-2 text-sm">
        <div className="flex justify-between">
          <dt className="text-[color:var(--cp-text-muted)]">类型</dt>
          <dd>{getClipTypeLabel(detail)}</dd>
        </div>
        <div className="flex justify-between">
          <dt className="text-[color:var(--cp-text-muted)]">创建时间</dt>
          <dd>{formatDateTime(detail.createdAt)}</dd>
        </div>
        {detail.sourceApp ? (
          <div className="flex justify-between">
            <dt className="text-[color:var(--cp-text-muted)]">来源</dt>
            <dd>{detail.sourceApp}</dd>
          </div>
        ) : null}
      </dl>
      <div className="mt-4 rounded-md bg-[rgba(var(--cp-surface1-rgb),0.1)] p-3">
        <p className="text-xs text-[color:var(--cp-text-muted)]">
          此类型的条目不支持直接编辑内容
        </p>
      </div>
    </div>
  );
}

function ConfirmDialog({
  pendingSelectId,
  onPendingSelectChange,
}: {
  pendingSelectId: string | null;
  onPendingSelectChange: (id: string | null) => void;
}) {
  const {
    draftText,
    isDirty,
    selectedItemId,
    setDraftText,
    setIsDirty,
    setMode,
    setSavedText,
    setSelectedItemId,
  } = useWorkbenchStore();
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const updateTextMutation = useUpdateTextMutation();

  useEffect(() => {
    if (!pendingSelectId) {
      setErrorMessage(null);
    }
  }, [pendingSelectId]);

  const handleSaveAndSwitch = async () => {
    if (!selectedItemId || !pendingSelectId) {
      return;
    }

    try {
      await updateTextMutation.mutateAsync({ id: selectedItemId, text: draftText });
      setIsDirty(false);
      setDraftText("");
      setSavedText("");
      setSelectedItemId(pendingSelectId);
      setMode("edit");
      setErrorMessage(null);
      onPendingSelectChange(null);
    } catch (error) {
      setErrorMessage(
        `保存后切换失败：${getErrorMessage(error, "当前修改仍未保存，请重试。")}`,
      );
    }
  };

  const handleDiscardAndSwitch = () => {
    if (!pendingSelectId) {
      return;
    }

    setIsDirty(false);
    setDraftText("");
    setSavedText("");
    setSelectedItemId(pendingSelectId);
    setMode("edit");
    setErrorMessage(null);
    onPendingSelectChange(null);
  };

  const handleCancel = () => {
    setErrorMessage(null);
    onPendingSelectChange(null);
  };

  if (!pendingSelectId || !isDirty) {
    return null;
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-80 rounded-lg bg-[color:var(--cp-window-shell)] p-6 shadow-xl">
        <h3 className="text-lg font-semibold text-[color:var(--cp-text-primary)]">未保存的修改</h3>
        <p className="mt-2 text-sm text-[color:var(--cp-text-secondary)]">
          当前有未保存的修改，是否保存后切换？
        </p>
        {errorMessage ? (
          <p className="mt-3 rounded-md bg-[rgba(var(--cp-red-rgb),0.08)] px-3 py-2 text-sm text-[color:var(--cp-danger)]">
            {errorMessage}
          </p>
        ) : null}
        <div className="mt-6 flex justify-end gap-2">
          <button
            className="rounded-md border border-[color:var(--cp-border-weak)] px-4 py-2 text-sm"
            onClick={handleCancel}
            type="button"
          >
            取消
          </button>
          <button
            className="rounded-md border border-[color:var(--cp-border-weak)] px-4 py-2 text-sm text-red-500"
            onClick={handleDiscardAndSwitch}
            type="button"
          >
            放弃修改
          </button>
          <button
            className="rounded-md bg-[color:var(--cp-accent-primary)] px-4 py-2 text-sm font-semibold text-cp-base disabled:opacity-50"
            disabled={updateTextMutation.isPending}
            onClick={() => void handleSaveAndSwitch()}
            type="button"
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
