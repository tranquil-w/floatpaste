import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { hideEditor as hideEditorWindow } from "../../bridge/commands";
import { EDITOR_SESSION_END_EVENT, EDITOR_SESSION_START_EVENT } from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import { useItemDetailQuery, useUpdateTextMutation } from "../../shared/queries/clipQueries";
import { useEditorStore, type EditorSession } from "./store";
import { getEditorKeyboardAction, moveFocusInDialog } from "./keyboard";

function getErrorMessage(error: unknown, fallback: string) {
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error;
  }

  return fallback;
}

function getSourceLabel(session: EditorSession | null) {
  if (!session) {
    return "等待编辑会话";
  }

  return session.source === "picker" ? "来自 Picker" : "来自搜索窗口";
}

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

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let offStart: (() => void) | undefined;
    let offEnd: (() => void) | undefined;
    let offCloseRequested: (() => void) | undefined;

    void listen<EditorSession>(EDITOR_SESSION_START_EVENT, (event) => {
      initializeSession({
        itemId: event.payload.itemId,
        source: event.payload.source,
        returnTo: event.payload.returnTo,
      });
    }).then((cleanup) => {
      offStart = cleanup;
    });

    void listen(EDITOR_SESSION_END_EVENT, () => {
      reset();
    }).then((cleanup) => {
      offEnd = cleanup;
    });

    void getCurrentWindow()
      .onCloseRequested((event) => {
        event.preventDefault();
        void requestCloseRef.current();
      })
      .then((cleanup) => {
        offCloseRequested = cleanup;
      });

    return () => {
      offStart?.();
      offEnd?.();
      offCloseRequested?.();
    };
  }, [initializeSession, reset]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const action = getEditorKeyboardAction({
        key: event.key,
        ctrlKey: event.ctrlKey,
        metaKey: event.metaKey,
        closeConfirmOpen,
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
        void saveCurrentText();
        return;
      }

      if (action === "confirm-cancel") {
        setCloseConfirmOpen(false);
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
  });

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

  useEffect(() => {
    if (detailQuery.data?.type === "text" && textareaRef.current) {
      textareaRef.current.focus();
      textareaRef.current.setSelectionRange(draftText.length, draftText.length);
    }
  }, [detailQuery.data?.id, detailQuery.data?.type, draftText.length]);

  useEffect(() => {
    if (closeConfirmOpen) {
      saveAndCloseButtonRef.current?.focus();
    }
  }, [closeConfirmOpen]);

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

  requestCloseRef.current = requestClose;

  async function handleSaveAndClose() {
    const success = await saveCurrentText();
    if (!success) {
      return;
    }

    await closeEditor();
  }

  const isTextItem = detailQuery.data?.type === "text";

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-[color:var(--cp-window-shell)] text-ink">
      <header className="flex shrink-0 items-center justify-between border-b border-[color:var(--cp-border-weak)] px-5 py-3">
        <div className="min-w-0">
          <p className="text-xs font-medium uppercase tracking-[0.16em] text-[color:var(--cp-text-muted)]">
            {getSourceLabel(session)}
          </p>
          <h1 className="mt-1 text-lg font-semibold text-[color:var(--cp-text-primary)]">
            独立编辑窗口
          </h1>
        </div>
        <button
          className="rounded-md border border-[color:var(--cp-border-weak)] px-3 py-1.5 text-sm hover:bg-[rgba(var(--cp-surface1-rgb),0.12)]"
          onClick={() => void requestClose()}
          type="button"
        >
          关闭
        </button>
      </header>

      {noticeMessage ? (
        <div className="border-b border-[rgba(var(--cp-green-rgb),0.16)] bg-[rgba(var(--cp-green-rgb),0.08)] px-5 py-2 text-sm text-[color:var(--cp-success)]">
          {noticeMessage}
        </div>
      ) : null}
      {errorMessage ? (
        <div className="border-b border-[rgba(var(--cp-red-rgb),0.16)] bg-[rgba(var(--cp-red-rgb),0.08)] px-5 py-2 text-sm text-[color:var(--cp-danger)]">
          {errorMessage}
        </div>
      ) : null}

      <main className="flex min-h-0 flex-1 flex-col px-5 py-4">
        {!session ? (
          <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
            等待编辑会话启动
          </div>
        ) : detailQuery.isLoading ? (
          <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
            正在加载条目内容...
          </div>
        ) : !detailQuery.data ? (
          <div className="flex h-full items-center justify-center text-sm text-[color:var(--cp-text-muted)]">
            未找到对应条目
          </div>
        ) : isTextItem ? (
          <textarea
            ref={textareaRef}
            className="h-full w-full resize-none rounded-md border border-[color:var(--cp-border-weak)] bg-cp-mantle px-5 py-5 text-[14px] leading-relaxed text-[color:var(--cp-text-primary)] outline-none transition-all duration-300 focus:border-[rgba(var(--cp-peach-rgb),0.35)] focus:bg-[color:var(--cp-window-shell)] focus:shadow-sm focus:shadow-[rgba(var(--cp-peach-rgb),0.08)] focus-visible:outline-none dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
            onChange={(event) => setDraftText(event.target.value)}
            placeholder="输入或编辑文本内容..."
            value={draftText}
          />
        ) : (
          <div className="rounded-xl border border-[color:var(--cp-border-weak)] bg-[rgba(var(--cp-surface1-rgb),0.08)] p-5">
            <h2 className="text-base font-semibold text-[color:var(--cp-text-primary)]">
              当前条目不支持文本编辑
            </h2>
            <p className="mt-2 text-sm leading-6 text-[color:var(--cp-text-secondary)]">
              仅文本条目可以进入独立编辑窗口。你可以关闭当前窗口并返回来源界面继续操作。
            </p>
          </div>
        )}
      </main>

      <footer className="flex shrink-0 items-center justify-between border-t border-[color:var(--cp-border-weak)] px-5 py-3">
        <div className="text-sm text-[color:var(--cp-text-muted)]">
          Enter 和方向键保留文本编辑语义，Ctrl+S 保存，Esc 请求关闭
        </div>
        <div className="flex items-center gap-2">
          <button
            className="rounded-md border border-[color:var(--cp-border-weak)] px-4 py-2 text-sm hover:bg-[rgba(var(--cp-surface1-rgb),0.12)]"
            onClick={() => void requestClose()}
            type="button"
          >
            关闭
          </button>
          <button
            className="rounded-md bg-[color:var(--cp-accent-primary)] px-4 py-2 text-sm font-semibold text-cp-base disabled:opacity-50"
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
            className="w-full max-w-sm rounded-2xl bg-[color:var(--cp-window-shell)] p-6 shadow-2xl"
            role="dialog"
          >
            <h2
              className="text-lg font-semibold text-[color:var(--cp-text-primary)]"
              id="editor-close-confirm-title"
            >
              发现未保存修改
            </h2>
            <p className="mt-2 text-sm leading-6 text-[color:var(--cp-text-secondary)]">
              你可以先保存当前内容再关闭，也可以放弃这次修改并直接返回来源窗口。
            </p>
            <div className="mt-6 flex justify-end gap-2">
              <button
                className="rounded-md border border-[color:var(--cp-border-weak)] px-4 py-2 text-sm"
                onClick={() => setCloseConfirmOpen(false)}
                type="button"
              >
                取消
              </button>
              <button
                className="rounded-md border border-[color:var(--cp-border-weak)] px-4 py-2 text-sm text-[color:var(--cp-danger)]"
                onClick={() => void closeEditor()}
                type="button"
              >
                放弃修改
              </button>
              <button
                ref={saveAndCloseButtonRef}
                className="rounded-md bg-[color:var(--cp-accent-primary)] px-4 py-2 text-sm font-semibold text-cp-base disabled:opacity-50"
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






