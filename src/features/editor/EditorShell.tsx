import { useEffect, useRef, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { deleteItem, hideEditor as hideEditorWindow } from "../../bridge/commands";
import { getImageUrl } from "../../bridge/imageUrl";
import { applyClipsChanged } from "../../shared/queries/clipsCache";
import { EDITOR_SESSION_END_EVENT, EDITOR_SESSION_START_EVENT } from "../../bridge/events";
import { getErrorMessage } from "../../shared/utils/error";
import { formatFileSize } from "../../shared/utils/clipDisplay";
import { formatDateTime } from "../../shared/utils/time";
import { isTauriRuntime } from "../../bridge/runtime";
import { useAppEvent } from "../../shared/hooks/useAppEvent";
import { useArmedConfirm } from "../../shared/hooks/useArmedConfirm";
import { LoadingSpinner } from "../../shared/ui/LoadingSpinner";
import { useItemDetailQuery, useUpdateTextMutation } from "../../shared/queries/clipQueries";
import type { ClipItemDetail } from "../../shared/types/clips";
import { useEditorStore, type EditorSession } from "./store";
import { getEditorKeyboardAction, isLocalEscapeTarget, moveFocusInDialog } from "./keyboard";
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
  // 两段式删除：待确认目标为当前条目 id，渲染为"确认删除"，超时或切换条目自动撤销
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

      // 本地组件（如标签输入框）正在消费 Esc（清空输入/收起浮层）时，窗口级关闭让位
      if (action === "request-close" && isLocalEscapeTarget(document.activeElement)) {
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

  // 图片条目的大图预览地址：resolve + asset 协议转换与搜索窗口共用同一实现，
  // 浏览器预览模式返回内置占位图，解析失败（文件丢失等）降级为占位文案
  const imageDetail = detailQuery.data?.type === "image" ? detailQuery.data : null;
  const [imagePreviewUrl, setImagePreviewUrl] = useState<string | null>(null);
  const [imagePreviewFailed, setImagePreviewFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setImagePreviewUrl(null);
    setImagePreviewFailed(false);
    if (!imageDetail?.imagePath) {
      return;
    }
    void getImageUrl(imageDetail.imagePath).then((url) => {
      if (cancelled) {
        return;
      }
      if (url) {
        setImagePreviewUrl(url);
      } else {
        setImagePreviewFailed(true);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [imageDetail?.id, imageDetail?.imagePath]);

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

  const detail = detailQuery.data;
  const isTextItem = detail?.type === "text";

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

      <main className="flex min-h-0 flex-1 flex-col">
        {!session ? (
          <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
            <svg
              aria-hidden="true"
              className="h-10 w-10 text-pg-fg-subtle"
              fill="none"
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="1.5"
              viewBox="0 0 24 24"
            >
              <rect height="4" rx="1" width="8" x="8" y="3" />
              <path d="M16 5h2a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h2" />
            </svg>
            <div>
              <p className="text-sm text-pg-fg-muted">选择一个文本条目开始编辑</p>
              <p className="mt-1.5 flex items-center justify-center gap-1 text-xs text-pg-fg-subtle">
                在速贴面板或搜索结果中选中文本后按
                <Kbd>Ctrl</Kbd>
                <span aria-hidden="true">+</span>
                <Kbd>Enter</Kbd>
              </p>
            </div>
          </div>
        ) : detailQuery.isLoading ? (
          <div className="flex h-full items-center justify-center">
            <LoadingSpinner size="sm" text="正在加载条目内容..." />
          </div>
        ) : !detail ? (
          <div className="flex h-full items-center justify-center text-sm text-pg-fg-muted">
            未找到对应条目
          </div>
        ) : (
          <div className="flex h-full min-h-0 flex-col px-5 pb-3 pt-4">
            <header className="flex shrink-0 items-center justify-between gap-3">
              <div className="flex min-w-0 flex-wrap items-center gap-x-2 text-xs text-pg-fg-subtle">
                {buildEditorMeta(detail, draftText).map((meta, index) => (
                  <span className="min-w-0" key={`${index}-${meta}`}>
                    {index > 0 ? <span aria-hidden="true">· </span> : null}
                    {meta}
                  </span>
                ))}
              </div>
              <button
                aria-label={deleteArmed ? "再次点击确认删除条目" : "删除条目"}
                className={`flex h-7 shrink-0 items-center justify-center rounded-md text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${
                  deleteArmed
                    ? "min-w-[4.25rem] bg-pg-danger-subtle px-2 text-pg-danger-fg"
                    : "w-7 text-pg-fg-muted hover:bg-pg-danger-subtle hover:text-pg-danger-fg"
                }`}
                disabled={!session}
                onClick={handleDeleteItem}
                title={deleteArmed ? "再次点击确认删除" : "删除条目"}
                type="button"
              >
                {deleteArmed ? (
                  "确认删除"
                ) : (
                  <svg
                    aria-hidden="true"
                    className="h-4 w-4"
                    fill="none"
                    stroke="currentColor"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth="1.8"
                    viewBox="0 0 24 24"
                  >
                    <path d="M4 7h16" />
                    <path d="M10 4h4a1 1 0 0 1 1 1v2H9V5a1 1 0 0 1 1-1Z" />
                    <path d="M6.5 7l.8 11.2A2 2 0 0 0 9.3 20h5.4a2 2 0 0 0 2-1.8L17.5 7" />
                    <path d="M10 11v5M14 11v5" />
                  </svg>
                )}
              </button>
            </header>
            {/* 标签区固定在主内容上方：建议浮层向下弹出时始终有内容区可覆盖 */}
            <div className="mt-2.5 shrink-0">
              <TagEditor
                itemId={detail.id}
                onError={(message) => {
                  setNoticeMessage(null);
                  setErrorMessage(message);
                }}
                tags={detail.tags}
              />
            </div>
            {isTextItem ? (
              <textarea
                ref={textareaRef}
                className="mt-2.5 h-full min-h-0 w-full flex-1 resize-none rounded-lg border border-transparent bg-pg-canvas-subtle px-5 py-4 text-[14px] leading-relaxed text-pg-fg-default outline-none transition-colors placeholder:text-pg-fg-subtle focus:border-pg-border-accent focus:bg-pg-canvas-inset focus-visible:outline-none"
                onChange={(event) => setDraftText(event.target.value)}
                placeholder="在此输入或修改文本..."
                value={draftText}
              />
            ) : (
              <div className="mt-2.5 flex min-h-0 flex-1 flex-col items-center justify-center gap-3 overflow-y-auto rounded-lg bg-pg-canvas-subtle p-4">
                {detail.type === "image" ? (
                  imagePreviewUrl ? (
                    <img
                      alt="条目图片预览"
                      className="max-h-full max-w-full rounded-md border border-pg-border-subtle object-contain"
                      decoding="async"
                      src={imagePreviewUrl}
                    />
                  ) : imagePreviewFailed ? (
                    <p className="text-sm text-pg-fg-subtle">图片预览不可用</p>
                  ) : (
                    <LoadingSpinner size="sm" text="正在加载图片..." />
                  )
                ) : detail.type === "file" && detail.filePaths.length > 0 ? (
                  <ul
                    className="w-full space-y-1 text-left text-[13px] text-pg-fg-muted"
                    role="list"
                  >
                    {detail.filePaths.map((path) => (
                      <li
                        className="truncate rounded-md bg-pg-canvas-default px-3 py-1.5"
                        key={path}
                        title={path}
                      >
                        {path}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="text-sm text-pg-fg-subtle">此条目内容不支持预览</p>
                )}
              </div>
            )}
          </div>
        )}
      </main>

      <footer className="flex shrink-0 items-center justify-between bg-pg-canvas-subtle px-5 py-2">
        <div className="flex min-w-0 items-center gap-3">
          {isDirty ? (
            <span className="flex shrink-0 items-center gap-1.5 text-xs font-medium text-pg-warning-fg">
              <span
                aria-hidden="true"
                className="h-1.5 w-1.5 rounded-full bg-pg-warning-emphasis"
              />
              未保存
            </span>
          ) : null}
          <span className="flex min-w-0 items-center gap-1 whitespace-nowrap text-[11px] text-pg-fg-muted">
            <Kbd>Ctrl+S</Kbd> 保存
            <span aria-hidden="true" className="px-0.5 text-pg-fg-subtle">
              ·
            </span>
            <Kbd>Esc</Kbd> 关闭
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <button
            className="rounded-md border border-pg-border-default px-4 py-2 text-sm hover:bg-pg-canvas-default"
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

/** 顶部元信息行：来源 · 时间，文本附字数、图片附尺寸、文件附个数与大小 */
function buildEditorMeta(detail: ClipItemDetail, draftText: string): string[] {
  const meta = [detail.sourceApp ?? "未知来源", formatDateTime(detail.createdAt)];

  if (detail.type === "text") {
    meta.push(`${Array.from(draftText).length} 字`);
    return meta;
  }

  if (detail.type === "image" && detail.imageWidth && detail.imageHeight) {
    meta.push(`${detail.imageWidth} × ${detail.imageHeight}`);
  }

  if (detail.type === "file" && detail.fileCount > 0) {
    meta.push(`${detail.fileCount} 个文件`);
  }

  const sizeLabel = formatFileSize(detail.fileSize ?? detail.totalSize);
  if (sizeLabel) {
    meta.push(sizeLabel);
  }

  return meta;
}

function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded border border-pg-border-subtle bg-pg-canvas-default px-1 font-mono text-[10px] leading-3 text-pg-fg-muted">
      {children}
    </kbd>
  );
}
