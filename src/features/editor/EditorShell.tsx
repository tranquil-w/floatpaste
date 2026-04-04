import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { hideEditor as hideEditorWindow } from "../../bridge/commands";
import { EDITOR_SESSION_END_EVENT, EDITOR_SESSION_START_EVENT } from "../../bridge/events";
import { getErrorMessage } from "../../shared/utils/error";
import { isTauriRuntime } from "../../bridge/runtime";
import { LoadingSpinner } from "../../shared/ui/LoadingSpinner";
import { useItemDetailQuery, useUpdateTextMutation } from "../../shared/queries/clipQueries";
import { useEditorStore, type EditorSession } from "./store";
import { getEditorKeyboardAction, moveFocusInDialog } from "./keyboard";

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
  saveCurrentTextRef.current = saveCurrentText;
  async function handleSaveAndClose() {
    const success = await saveCurrentText();
    if (!success) {
      return;
    }

    await closeEditor();
  }

  handleSaveAndCloseRef.current = handleSaveAndClose;
  closeConfirmOpenRef.current = closeConfirmOpen;

  const isTextItem = detailQuery.data?.type === "text";

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-pg-canvas-default text-pg-fg-default">
      <div className="h-[3px] w-full bg-gradient-to-r from-pg-blue-5 to-pg-blue-4 shrink-0" />

      {noticeMessage ? (
        <div className="bg-pg-success-subtle px-5 py-2 text-sm text-pg-success-fg">
          {noticeMessage}
        </div>
      ) : null}
      {errorMessage ? (
        <div className="bg-pg-danger-subtle px-5 py-2 text-sm text-pg-danger-fg">
          {errorMessage}
        </div>
      ) : null}

      <main className="flex min-h-0 flex-1 flex-col px-5 py-4">
        {!session ? (
          <div className="flex h-full flex-col items-center justify-center gap-1">
            <p className="text-sm text-pg-fg-muted">选择一个文本条目开始编辑</p>
            <p className="text-xs text-pg-fg-subtle">在速贴面板或搜索结果中，选中文本后按 Ctrl+Enter</p>
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
          <textarea
            ref={textareaRef}
            className="h-full w-full resize-none rounded-md border border-pg-border-default bg-pg-canvas-subtle px-5 py-5 text-[14px] leading-relaxed text-pg-fg-default outline-none transition-colors focus:border-pg-accent-fg focus:bg-pg-canvas-inset focus-visible:outline-none"
            onChange={(event) => setDraftText(event.target.value)}
            placeholder="在此输入或修改文本..."
            value={draftText}
          />
        ) : (
          <div className="rounded-lg border border-pg-border-default bg-pg-canvas-subtle p-5">
            <h2 className="text-base font-semibold text-pg-fg-muted">
              此条目无法编辑
            </h2>
            <p className="mt-2 text-sm leading-6 text-pg-fg-muted">
              只有文本类型的条目可以在这里编辑。你可以关闭当前窗口并返回来源界面继续操作。
            </p>
          </div>
        )}
      </main>

      <footer className="flex shrink-0 items-center justify-between border-t border-pg-border-muted px-5 py-3">
        <div className="text-sm text-pg-fg-subtle">
          Ctrl+S 保存 · Esc 关闭
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
            className="w-full max-w-sm rounded-2xl bg-pg-canvas-default p-6 shadow-pg-xl"
            role="dialog"
          >
            <h2
              className="text-lg font-semibold text-pg-fg-default"
              id="editor-close-confirm-title"
            >
              有未保存的修改
            </h2>
            <p className="mt-2 text-sm leading-6 text-pg-fg-muted">
              保存修改还是放弃并关闭？
            </p>
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






