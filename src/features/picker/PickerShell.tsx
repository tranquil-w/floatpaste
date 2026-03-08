import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { queryClient } from "../../app/queryClient";
import { hidePicker, openManager, pasteItem } from "../../bridge/commands";
import {
  CLIPS_CHANGED_EVENT,
  PICKER_CONFIRM_EVENT,
  PICKER_NAVIGATE_EVENT,
  PICKER_SELECT_INDEX_EVENT,
  PICKER_SESSION_START_EVENT,
} from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import type { ClipItemSummary } from "../../shared/types/clips";
import { formatDateTime } from "../../shared/utils/time";
import { usePickerFavoritesQuery, usePickerRecentQuery } from "./queries";

function mergePickerItems(favorites: ClipItemSummary[], recent: ClipItemSummary[]): ClipItemSummary[] {
  const merged = new Map<string, ClipItemSummary>();

  favorites.forEach((item) => merged.set(item.id, item));
  recent.forEach((item) => {
    if (!merged.has(item.id)) {
      merged.set(item.id, item);
    }
  });

  return Array.from(merged.values()).slice(0, 9);
}

export function PickerShell() {
  const favorites = usePickerFavoritesQuery();
  const recent = usePickerRecentQuery();
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [lastMessage, setLastMessage] = useState("");

  const items = useMemo(
    () => mergePickerItems(favorites.data ?? [], recent.data ?? []),
    [favorites.data, recent.data],
  );

  const openManagerFromPicker = async () => {
    await hidePicker();
    await openManager();
  };

  const confirmSelection = async (index: number) => {
    const item = items[index];
    if (!item) {
      return;
    }

    const result = await pasteItem(item.id, { restoreClipboardAfterPaste: true });
    setLastMessage(result.message);
  };

  useEffect(() => {
    if (selectedIndex >= items.length) {
      setSelectedIndex(0);
    }
  }, [items.length, selectedIndex]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let disposed = false;
    let unlistenStart: (() => void) | undefined;
    let unlistenClips: (() => void) | undefined;
    let unlistenNavigate: (() => void) | undefined;
    let unlistenConfirm: (() => void) | undefined;
    let unlistenSelectIndex: (() => void) | undefined;

    void listen(PICKER_SESSION_START_EVENT, async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["picker-favorites"] }),
        queryClient.invalidateQueries({ queryKey: ["picker-recent"] }),
      ]);

      if (!disposed) {
        setSelectedIndex(0);
        setLastMessage("");
      }
    }).then((cleanup) => {
      unlistenStart = cleanup;
    });

    void listen(CLIPS_CHANGED_EVENT, async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["picker-favorites"] }),
        queryClient.invalidateQueries({ queryKey: ["picker-recent"] }),
      ]);
    }).then((cleanup) => {
      unlistenClips = cleanup;
    });

    void listen<string>(PICKER_NAVIGATE_EVENT, async (event) => {
      if (disposed || !items.length) {
        return;
      }

      setSelectedIndex((current) => {
        if (event.payload === "up") {
          return (current - 1 + items.length) % items.length;
        }

        return (current + 1) % items.length;
      });
    }).then((cleanup) => {
      unlistenNavigate = cleanup;
    });

    void listen(PICKER_CONFIRM_EVENT, async () => {
      if (disposed) {
        return;
      }

      await confirmSelection(selectedIndex);
    }).then((cleanup) => {
      unlistenConfirm = cleanup;
    });

    void listen<number>(PICKER_SELECT_INDEX_EVENT, async (event) => {
      if (disposed || !items.length) {
        return;
      }

      const index = Math.max(0, Math.min(event.payload, items.length - 1));
      setSelectedIndex(index);
      await confirmSelection(index);
    }).then((cleanup) => {
      unlistenSelectIndex = cleanup;
    });

    return () => {
      disposed = true;
      unlistenStart?.();
      unlistenClips?.();
      unlistenNavigate?.();
      unlistenConfirm?.();
      unlistenSelectIndex?.();
    };
  }, [items.length, selectedIndex]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (!items.length && event.key !== "Escape") {
        return;
      }

      if (event.key === "Escape") {
        event.preventDefault();
        void hidePicker();
        return;
      }

      if (event.key === "ArrowUp") {
        event.preventDefault();
        setSelectedIndex((current) => (current - 1 + items.length) % items.length);
        return;
      }

      if (event.key === "ArrowDown") {
        event.preventDefault();
        setSelectedIndex((current) => (current + 1) % items.length);
        return;
      }

      if (event.key === "Enter") {
        event.preventDefault();
        void confirmSelection(selectedIndex);
        return;
      }

      if (/^[1-9]$/.test(event.key)) {
        event.preventDefault();
        const index = Math.min(Number(event.key) - 1, items.length - 1);
        if (index >= 0) {
          setSelectedIndex(index);
          void confirmSelection(index);
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [items.length, selectedIndex, items]);

  return (
    <div className="min-h-screen bg-transparent px-4 py-4 text-ink">
      <div className="mx-auto max-w-[560px] rounded-[28px] border border-white/70 bg-[rgba(252,249,244,0.96)] p-4 shadow-[0_24px_80px_rgba(15,23,42,0.28)] backdrop-blur">
        <div className="flex items-start justify-between gap-4">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.28em] text-accentDeep">速贴面板</p>
            <h1 className="mt-2 font-display text-3xl leading-none">FloatPaste Picker</h1>
            <p className="mt-2 text-sm text-slate-600">
              使用方向键、数字键、回车和 Esc 完成选择。若需完整搜索或编辑，点右上角进入资料库。
            </p>
          </div>
          <button
            className="rounded-2xl bg-slate-900 px-3 py-2 text-sm font-semibold text-white"
            onClick={() => void openManagerFromPicker()}
            type="button"
          >
            资料库
          </button>
        </div>

        <div className="mt-4 grid gap-3">
          {items.length ? (
            items.map((item, index) => {
              const isSelected = index === selectedIndex;
              return (
                <button
                  className={`w-full rounded-3xl border px-4 py-4 text-left transition ${
                    isSelected
                      ? "border-accent bg-amber-50 shadow-sm"
                      : "border-slate-200 bg-white/80 hover:border-slate-300"
                  }`}
                  key={item.id}
                  onClick={() => setSelectedIndex(index)}
                  onDoubleClick={() => {
                    void confirmSelection(index);
                  }}
                  type="button"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div>
                      <p className="text-xs font-semibold uppercase tracking-[0.22em] text-slate-400">
                        {index + 1}
                      </p>
                      <p className="mt-2 line-clamp-2 text-sm leading-6 text-slate-800">{item.contentPreview}</p>
                    </div>
                    {item.isFavorited ? (
                      <span className="rounded-full bg-amber-100 px-2 py-1 text-xs font-semibold text-amber-700">
                        收藏
                      </span>
                    ) : null}
                  </div>
                  <div className="mt-3 flex flex-wrap gap-3 text-xs text-slate-500">
                    <span>{item.sourceApp ?? "未知来源"}</span>
                    <span>{formatDateTime(item.lastUsedAt ?? item.createdAt)}</span>
                  </div>
                </button>
              );
            })
          ) : (
            <div className="rounded-3xl border border-dashed border-slate-300 bg-white/80 px-4 py-10 text-center text-sm text-slate-600">
              暂无历史记录。先复制一段文本，再按全局快捷键唤起速贴面板。
            </div>
          )}
        </div>

        <div className="mt-4 rounded-2xl bg-slate-900 px-4 py-3 text-xs text-white/80">
          <div className="flex flex-wrap gap-x-4 gap-y-2">
            <span>↑ / ↓ 选择</span>
            <span>1-9 直选</span>
            <span>Enter 粘贴</span>
            <span>Esc 关闭</span>
            <span>右上角进入资料库</span>
          </div>
          {lastMessage ? <p className="mt-2 text-white">{lastMessage}</p> : null}
        </div>
      </div>
    </div>
  );
}
