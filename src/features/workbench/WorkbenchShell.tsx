import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useWorkbenchStore } from "./store";
import { WORKBENCH_SESSION_START_EVENT, WORKBENCH_SESSION_END_EVENT, WORKBENCH_NAVIGATE_EVENT } from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import { useWorkbenchRecentQuery, useWorkbenchSearchQuery } from "./queries";
import {
  useItemDetailQuery,
  useUpdateTextMutation,
  useSetFavoritedMutation,
  useDeleteItemMutation,
  usePasteMutation,
} from "../manager/queries";
import { hideWorkbench } from "../../bridge/commands";
import type { ClipItemSummary, ClipItemDetail } from "../../shared/types/clips";
import { getClipTypeLabel } from "../../shared/utils/clipDisplay";
import { formatDateTime } from "../../shared/utils/time";

type WorkbenchSource = "picker_edit" | "picker_search" | "global";

function parseSource(raw: string): WorkbenchSource {
  if (raw === "picker_edit" || raw === "picker_search" || raw === "global") {
    return raw;
  }
  return "global";
}

export function WorkbenchShell() {
  const { setSession, setMode, setSelectedItemId, setKeyword } = useWorkbenchStore();
  const [pendingSelectId, setPendingSelectId] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauriRuntime()) return;

    let offStart: (() => void) | undefined;
    let offEnd: (() => void) | undefined;
    let offNavigate: (() => void) | undefined;

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
    }).then((cleanup) => { offStart = cleanup; }).catch((err: unknown) => {
      console.error("监听 WORKBENCH_SESSION_START_EVENT 失败:", err);
    });

    void listen(WORKBENCH_SESSION_END_EVENT, () => {
      setSession(null);
      setMode("search");
      setSelectedItemId(null);
      setKeyword("");
    }).then((cleanup) => { offEnd = cleanup; }).catch((err: unknown) => {
      console.error("监听 WORKBENCH_SESSION_END_EVENT 失败:", err);
    });

    void listen<string>(WORKBENCH_NAVIGATE_EVENT, (_event) => {
      // TODO: 实现键盘导航（上/下方向键选中列表项）
    }).then((cleanup) => { offNavigate = cleanup; }).catch((err: unknown) => {
      console.error("监听 WORKBENCH_NAVIGATE_EVENT 失败:", err);
    });

    return () => {
      offStart?.();
      offEnd?.();
      offNavigate?.();
    };
  }, [setSession, setMode, setSelectedItemId, setKeyword]);

  return (
    <div className="h-screen w-screen overflow-hidden bg-[color:var(--cp-window-shell)] text-ink">
      <div className="flex h-full flex-col">
        <header className="shrink-0 border-b border-[color:var(--cp-border-weak)] px-4 py-3">
          <TopBar />
        </header>
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

function TopBar() {
  const { session, keyword, setKeyword } = useWorkbenchStore();
  return (
    <div className="flex items-center gap-4">
      <span className="text-xs text-[color:var(--cp-text-muted)]">
        {session?.source === "picker_edit" && "从 Picker 编辑"}
        {session?.source === "picker_search" && "从 Picker 搜索"}
        {session?.source === "global" && "全局搜索"}
      </span>
      <input
        className="flex-1 rounded-md border border-[color:var(--cp-border-weak)] bg-[color:var(--cp-control-surface)] px-3 py-2 text-sm outline-none focus:border-[rgba(var(--cp-peach-rgb),0.35)]"
        placeholder="搜索记录..."
        value={keyword}
        onChange={(e) => setKeyword(e.target.value)}
      />
      <button
        className="rounded-md border border-[color:var(--cp-border-weak)] px-3 py-1.5 text-sm hover:bg-[rgba(var(--cp-surface1-rgb),0.1)]"
        onClick={() => void hideWorkbench()}
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
  const { keyword, selectedItemId, setSelectedItemId, setMode, isDirty, setIsDirty, setSavedText } = useWorkbenchStore();

  const hasKeyword = keyword.trim().length > 0;
  const recent = useWorkbenchRecentQuery(!hasKeyword);
  const search = useWorkbenchSearchQuery(keyword, hasKeyword);

  const items: ClipItemSummary[] = hasKeyword
    ? (search.data?.items ?? [])
    : (recent.data ?? []);

  const isLoading = hasKeyword ? search.isLoading : recent.isLoading;

  const handleSelectItem = (item: ClipItemSummary) => {
    if (isDirty && selectedItemId) {
      // 有未保存修改，弹出确认框
      onPendingSelect(item.id);
      return;
    }
    setSelectedItemId(item.id);
    setMode("edit");
    setIsDirty(false);
    setSavedText("");
  };

  // 抑制未使用警告（pendingSelectId 传入仅供未来高亮使用）
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
            {item.isFavorited && <span className="text-[color:var(--cp-favorite)]">★</span>}
          </div>
        </button>
      ))}
    </div>
  );
}

function EditPanel() {
  const {
    selectedItemId,
    draftText,
    setDraftText,
    isDirty,
    setIsDirty,
    savedText,
    setSavedText,
    setSelectedItemId,
    setMode,
  } = useWorkbenchStore();

  const detail = useItemDetailQuery(selectedItemId);
  const updateTextMutation = useUpdateTextMutation();
  const favoritedMutation = useSetFavoritedMutation();
  const deleteMutation = useDeleteItemMutation();
  const pasteMutation = usePasteMutation();

  // 同步详情到编辑区
  useEffect(() => {
    if (detail.data && !isDirty) {
      setDraftText(detail.data.textContent ?? "");
      setSavedText(detail.data.textContent ?? "");
    }
  }, [detail.data, isDirty, setDraftText, setSavedText]);

  // 检测脏状态
  useEffect(() => {
    setIsDirty(draftText !== savedText);
  }, [draftText, savedText, setIsDirty]);

  const handleSave = async () => {
    if (!selectedItemId) return;
    try {
      await updateTextMutation.mutateAsync({ id: selectedItemId, text: draftText });
      setSavedText(draftText);
      setIsDirty(false);
    } catch (err) {
      console.error("保存失败:", err);
    }
  };

  const handleToggleFavorite = async () => {
    if (!selectedItemId || !detail.data) return;
    try {
      await favoritedMutation.mutateAsync({
        id: selectedItemId,
        value: !detail.data.isFavorited,
      });
    } catch (err) {
      console.error("收藏操作失败:", err);
    }
  };

  const handleDelete = async () => {
    if (!selectedItemId) return;
    try {
      await deleteMutation.mutateAsync(selectedItemId);
      setSelectedItemId(null);
      setMode("search");
      setIsDirty(false);
      setSavedText("");
    } catch (err) {
      console.error("删除失败:", err);
    }
  };

  const handlePaste = async () => {
    if (!selectedItemId) return;

    // 检查未保存修改
    if (isDirty) {
      // TODO: 弹出确认对话框
      return;
    }

    try {
      const result = await pasteMutation.mutateAsync({
        id: selectedItemId,
        option: { restoreClipboardAfterPaste: true, pasteToTarget: true },
      });

      if (result.success) {
        await hideWorkbench();
      }
    } catch (err) {
      console.error("回贴失败:", err);
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

  const isTextItem = detail.data.clipType === "text";

  return (
    <div className="flex h-full flex-col">
      {/* 编辑区或元信息区 */}
      <div className="min-h-0 flex-1 p-4">
        {isTextItem ? (
          <textarea
            className="h-full w-full resize-none rounded-md border border-[color:var(--cp-border-weak)] bg-[color:var(--cp-control-surface)] p-4 text-sm outline-none focus:border-[rgba(var(--cp-peach-rgb),0.35)]"
            value={draftText}
            onChange={(e) => setDraftText(e.target.value)}
            placeholder="输入内容..."
          />
        ) : (
          <MetaInfoPanel detail={detail.data} />
        )}
      </div>

      {/* 操作按钮区 */}
      <div className="shrink-0 border-t border-[color:var(--cp-border-weak)] px-4 py-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <button
              className="rounded-md bg-[color:var(--cp-accent-primary)] px-4 py-2 text-sm font-semibold text-cp-base disabled:opacity-50"
              onClick={() => void handleSave()}
              disabled={!isDirty || updateTextMutation.isPending}
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
        {detail.sourceApp && (
          <div className="flex justify-between">
            <dt className="text-[color:var(--cp-text-muted)]">来源</dt>
            <dd>{detail.sourceApp}</dd>
          </div>
        )}
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
  const { isDirty, setIsDirty, setSavedText, selectedItemId, setMode, setSelectedItemId } = useWorkbenchStore();
  const updateTextMutation = useUpdateTextMutation();

  const handleSaveAndSwitch = async () => {
    if (!selectedItemId || !pendingSelectId) return;
    try {
      const { draftText } = useWorkbenchStore.getState();
      await updateTextMutation.mutateAsync({ id: selectedItemId, text: draftText });
      setIsDirty(false);
      setSavedText(draftText);
      setSelectedItemId(pendingSelectId);
      setMode("edit");
      onPendingSelectChange(null);
    } catch (err) {
      console.error("保存并切换失败:", err);
    }
  };

  const handleDiscardAndSwitch = () => {
    if (!pendingSelectId) return;
    setIsDirty(false);
    setSelectedItemId(pendingSelectId);
    setMode("edit");
    onPendingSelectChange(null);
  };

  const handleCancel = () => {
    onPendingSelectChange(null);
  };

  if (!pendingSelectId) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-80 rounded-lg bg-[color:var(--cp-window-shell)] p-6 shadow-xl">
        <h3 className="text-lg font-semibold text-[color:var(--cp-text-primary)]">未保存的修改</h3>
        <p className="mt-2 text-sm text-[color:var(--cp-text-secondary)]">
          当前有未保存的修改，是否保存后切换？
        </p>
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
            onClick={() => void handleSaveAndSwitch()}
            disabled={updateTextMutation.isPending}
            type="button"
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
