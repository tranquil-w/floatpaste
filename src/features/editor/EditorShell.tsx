import { useEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { deleteItem, hideEditor as hideEditorWindow } from "../../bridge/commands";
import { applyClipsChanged } from "../../shared/queries/clipsCache";
import { EDITOR_SESSION_END_EVENT, EDITOR_SESSION_START_EVENT } from "../../bridge/events";
import { getErrorMessage } from "../../shared/utils/error";
import { isTauriRuntime } from "../../bridge/runtime";
import { useAppEvent } from "../../shared/hooks/useAppEvent";
import { useArmedConfirm } from "../../shared/hooks/useArmedConfirm";
import { LoadingSpinner } from "../../shared/ui/LoadingSpinner";
import { useItemDetailQuery, useUpdateTextMutation } from "../../shared/queries/clipQueries";
import { useEditorStore, type EditorSession } from "./store";
import { getEditorKeyboardAction, moveFocusInDialog } from "./keyboard";
import { TagEditor } from "./tagEditor";

export function EditorShell() {
  const {
    closeConfirmOpen,
    draftText,
    errorMessage,
    initializeSession,
    isDirty,
    markSaved,
    noticeMessage,
    reset,
    savedText,
    session,
    setCloseConfirmOpen,
    setDraftText,
    setErrorMessage,
    setNoticeMessage,
    syncText,
  } = useEditorStore();
  const detailQuery = useItemDetailQuery(session?.itemId ?? null);
  const updateTextMutation = useUpdateTextMutation();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const saveAndCloseButtonRef = useRef<HTMLButtonElement>(null);
  const requestCloseRef = useRef<() => Promise<void>>(async () => {});
  const saveCurrentTextRef = useRef<() => Promise<boolean>>(async () => false);
  const handleSaveAndCloseRef = useRef<() => Promise<void>>(async () => {});
  const closeConfirmOpenRef = useRef(closeConfirmOpen);
  const noticeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 两段式删除：待确认目标为当前条目 id，渲染为“确认删除”，超时或切换条目自动撤销
  const {
    armedTarget: deleteArmedItemId,
    request: requestArmedDelete,
    reset: resetDeleteArmed,
  } = useArmedConfirm<string>(async (id) => {
    try {
      await deleteItem(id);
      applyClipsChanged({ kind: "deleted", id });
      await closeEditor();
    } catch (error) {
      setNoticeMessage(null);
      setErrorMessage(`删除失败：${getErrorMessage(error, "请稍后重试。")}`);
    }
  });
  const deleteArmed = session ? deleteArmedItemId === session.itemId : false;

  useAppEvent<EditorSession>(EDITOR_SESSION_START_EVENT, (payload) => {
    initializeSession({
      itemId: payload.itemId,
      source: payload.source,
      returnTo: payload.returnTo,
    });
  });

  useAppEvent(EDITOR_SESSION_END_EVENT, () => {
    reset();
  });

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let offCloseRequested: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested((event) => {
        event.preventDefault();
        void requestCloseRef.current();
      })
      .then((cleanup) => {
        offCloseRequested = cleanup;
      });

    return () => {
      offCloseRequested?.();
    };
  }, []);

  // handler 只读 ref 与稳定的 store setter，依赖为空避免每轮渲染重挂监听
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const action = getEditorKeyboardAction({
        key: event.key,
        ctrlKey: event.ctrlKey,
        metaKey: event.metaKey,
        closeConfirmOpen: closeConfirmOpenRef.current,
      });

      if (!action) {
        return;
      }

      event.preventDefault();

      if (action === "request-close") {
        void requestCloseRef.current();
        return;
      }

      if (action === "save") {
        void saveCurrentTextRef.current();
        return;
      }

      if (action === "confirm-cancel") {
        setCloseConfirmOpen(false);
        return;
      }

      if (action === "confirm-primary") {
        void handleSaveAndCloseRef.current();
        return;
      }

      moveFocusInDialog({
        activeElement: document.activeElement,
        container: dialogRef.current,
        shiftKey: event.shiftKey,
      });
    };

    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, []);

  useEffect(() => {
    if (!detailQuery.data || detailQuery.data.id !== session?.itemId) {
      return;
    }

    if (detailQuery.data.type !== "text") {
      if (!isDirty) {
        syncText("");
      }
      return;
    }

    if (!isDirty || savedText === "") {
      syncText(detailQuery.data.fullText ?? "");
    }
  }, [detailQuery.data, isDirty, savedText, session?.itemId, syncText]);

  const caretPlacedItemIdRef = useRef<string | null>(null);

  // 仅在条目首次载入时把光标定位到末尾；依赖里不能含 draftText，
  // 否则编辑过程中每次按键都会把光标强制甩到末尾
  useEffect(() => {
    if (detailQuery.data?.type !== "text" || !textareaRef.current) {
      return;
    }
    if (caretPlacedItemIdRef.current === detailQuery.data.id) {
      return;
    }
    caretPlacedItemIdRef.current = detailQuery.data.id;
    const textarea = textareaRef.current;
    // 等文本同步 effect 触发的 re-render commit 后再定位，避免读到上一条目的旧文本
    requestAnimationFrame(() => {
      textarea.focus();
      textarea.setSelectionRange(textarea.value.length, textarea.value.length);
    });
  }, [detailQuery.data?.id, detailQuery.data?.type]);

  useEffect(() => {
    if (closeConfirmOpen) {
      saveAndCloseButtonRef.current?.focus();
    }
  }, [closeConfirmOpen]);

  // 顶部通知（如"已保存"）短暂展示后自动消失，避免长期占用顶部空间
  useEffect(() => {
    if (noticeTimerRef.current) {
      clearTimeout(noticeTimerRef.current);
      noticeTimerRef.current = null;
    }
    if (!noticeMessage) {
      return;
    }
    noticeTimerRef.current = setTimeout(() => {
      setNoticeMessage(null);
    }, 3000);
    return () => {
      if (noticeTimerRef.current) {
        clearTimeout(noticeTimerRef.current);
        noticeTimerRef.current = null;
      }
    };
  }, [noticeMessage, setNoticeMessage]);

  // 切换条目时撤销待确认删除，避免新条目首次点击被旧确认态直接放行
  useEffect(() => {
    resetDeleteArmed();
  }, [session?.itemId]);

  async function saveCurrentText() {
    if (!session || detailQuery.data?.type !== "text") {
      return false;
    }

    try {
      await updateTextMutation.mutateAsync({
        id: session.itemId,
        text: draftText,
      });
      markSaved(draftText);
      setNoticeMessage("已保存当前修改");
      setErrorMessage(null);
      return true;
    } catch (error) {
      setNoticeMessage(null);
      setErrorMessage(`保存失败：${getErrorMessage(error, "请稍后重试。")}`);
      return false;
    }
  }

  async function closeEditor() {
    try {
      await hideEditorWindow();
      setCloseConfirmOpen(false);
      setErrorMessage(null);
    } catch (error) {
      setErrorMessage(`关闭编辑器失败：${getErrorMessage(error, "请稍后重试。")}`);
    }
  }

  async function requestClose() {
    if (isDirty) {
      setCloseConfirmOpen(true);
      return;
    }

    await closeEditor();
  }

  // 两段式删除：首次点击进入待确认态，超时自动退出；确认后删除条目并关闭编辑器
  function handleDeleteItem() {
    if (!session) {
      return;
    }

    requestArmedDelete(session.itemId);
  }

  async function handleSaveAndClose() {
    const success = await saveCurrentText();
    if (!success) {
      return;
    }

    await closeEditor();
  }

  // 回调依赖当轮渲染的 state，渲染后统一同步到 ref 供键盘监听读取
  useEffect(() => {
    requestCloseRef.current = requestClose;
    saveCurrentTextRef.current = saveCurrentText;
    handleSaveAndCloseRef.current = handleSaveAndClose;
    closeConfirmOpenRef.current = closeConfirmOpen;
  });

  const isTextItem = detailQuery.data?.type === "text";

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-pg-canvas-default text-pg-fg-default">
      {noticeMessage ? (
        <div className="bg-pg-success-subtle px-5 py-2 text-sm text-pg-success-fg" role="status">
          {noticeMessage}
        </div>
      ) : null}
      {errorMessage ? (
        <div className="bg-pg-danger-subtle px-5 py-2 text-sm text-pg-danger-fg" role="alert">
          {errorMessage}
        </div>
      ) : null}

      <main className="flex min-h-0 flex-1 flex-col px-5 py-4">
        {!session ? (
          <div className="flex h-full flex-col items-center justify-center gap-1">
            <p className="text-sm text-pg-fg-muted">选择一个文本条目开始编辑</p>
            <p className="text-xs text-pg-fg-subtle">
              在速贴面板或搜索结果中，选中文本后按 Ctrl+Enter
            </p>
          </div>
        ) : detailQuery.isLoading ? (
          <div className="flex h-full items-center justify-center">
            <LoadingSpinner size="sm" text="正在加载条目内容..." />
          </div>
        ) : !detailQuery.data ? (
          <div className="flex h-full items-center justify-center text-sm text-pg-fg-muted">
            未找到对应条目
          </div>
        ) : isTextItem ? (
          <div className="flex h-full min-h-0 flex-col gap-3">
            <TagEditor
              itemId={detailQuery.data.id}
              tags={detailQuery.data.tags}
              onError={(message) => {
                setNoticeMessage(null);
                setErrorMessage(message);
              }}
            />
            <textarea
              ref={textareaRef}
              className="h-full w-full min-h-0 flex-1 resize-none rounded-md border border-pg-border-default bg-pg-canvas-subtle px-5 py-5 text-[14px] leading-relaxed text-pg-fg-default outline-none transition-colors focus:border-pg-accent-fg focus:bg-pg-canvas-inset focus-visible:outline-none"
              onChange={(event) => setDraftText(event.target.value)}
              placeholder="在此输入或修改文本..."
              value={draftText}
            />
          </div>
        ) : (
          <div className="rounded-lg border border-pg-border-default bg-pg-canvas-subtle p-5">
            <h2 className="text-base font-semibold text-pg-fg-muted">
              此条目内容不支持编辑
            </h2>
            <p className="mt-2 text-sm leading-6 text-pg-fg-muted">
              只有文本类型的条目可以编辑内容。你仍可以在下方为它管理标签。
            </p>
            <div className="mt-4">
              <TagEditor
                itemId={detailQuery.data.id}
                tags={detailQuery.data.tags}
                onError={(message) => {
                  setNoticeMessage(null);
                  setErrorMessage(message);
                }}
              />
            </div>
          </div>
        )}
      </main>

      <footer className="flex shrink-0 items-center justify-between border-t border-pg-border-muted px-5 py-3">
        <div className="flex items-center gap-3">
          <button
            className={`rounded-md border px-3 py-2 text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${
              deleteArmed
                ? "border-pg-danger-fg bg-pg-danger-subtle text-pg-danger-fg"
                : "border-pg-border-default text-pg-fg-muted hover:border-pg-danger-fg hover:text-pg-danger-fg"
            }`}
            disabled={!session}
            onClick={() => void handleDeleteItem()}
            type="button"
          >
            {deleteArmed ? "确认删除" : "删除条目"}
          </button>
          <span className="text-sm text-pg-fg-subtle">Ctrl+S 保存 · Esc 关闭</span>
        </div>
        <div className="flex items-center gap-2">
          <button
            className="rounded-md border border-pg-border-default px-4 py-2 text-sm hover:bg-pg-canvas-subtle"
            onClick={() => void requestClose()}
            type="button"
          >
            关闭
          </button>
          <button
            className="rounded-md bg-pg-accent-emphasis px-4 py-2 text-sm font-semibold text-pg-fg-on-emphasis disabled:opacity-50"
            disabled={!isTextItem || !isDirty || updateTextMutation.isPending}
            onClick={() => void saveCurrentText()}
            type="button"
          >
            保存
          </button>
        </div>
      </footer>

      {closeConfirmOpen ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 px-4">
          <div
            ref={dialogRef}
            aria-labelledby="editor-close-confirm-title"
            aria-modal="true"
            className="w-full max-w-sm rounded-xl bg-pg-canvas-default p-6 shadow-pg-xl"
            role="dialog"
          >
            <h2
              className="text-lg font-semibold text-pg-fg-default"
              id="editor-close-confirm-title"
            >
              有未保存的修改
            </h2>
            <p className="mt-2 text-sm leading-6 text-pg-fg-muted">保存修改还是放弃并关闭？</p>
            <div className="mt-6 flex justify-end gap-2">
              <button
                className="rounded-md border border-pg-border-default px-4 py-2 text-sm"
                onClick={() => setCloseConfirmOpen(false)}
                type="button"
              >
                取消
              </button>
              <button
                className="rounded-md border border-pg-border-default px-4 py-2 text-sm text-pg-danger-fg"
                onClick={() => void closeEditor()}
                type="button"
              >
                放弃修改
              </button>
              <button
                ref={saveAndCloseButtonRef}
                className="rounded-md bg-pg-accent-emphasis px-4 py-2 text-sm font-semibold text-pg-fg-on-emphasis disabled:opacity-50"
                disabled={updateTextMutation.isPending}
                onClick={() => void handleSaveAndClose()}
                type="button"
              >
                保存并关闭
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
