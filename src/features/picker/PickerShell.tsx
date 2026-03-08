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
    <div className="flex h-screen items-start justify-center bg-transparent px-4 py-4 text-ink">
      <div className="flex max-h-full w-full max-w-[560px] flex-col rounded-[28px] border border-white/70 bg-[rgba(252,249,244,0.96)] p-4 shadow-[0_24px_80px_rgba(15,23,42,0.28)] backdrop-blur">
        <div className="flex shrink-0 items-start justify-between gap-4">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.28em] text-accentDeep">速贴面板</p>
            <h1 className="mt-2 font-display text-3xl leading-none">FloatPaste Picker</h1>
            <p className="mt-2 text-sm text-slate-600">
              使用方向键、数字键、回车和 Esc 完成选择。若需完整搜索或编辑，点右上角进入资料库。
            </p>
          </div>
          <button
            className="whitespace-nowrap rounded-2xl bg-slate-900 px-3 py-2 text-sm font-semibold text-white"
            onClick={() => void openManagerFromPicker()}
            type="button"
          >
            资料库
          </button>
        </div>

        <div className="mt-4 flex min-h-0 flex-1 flex-col">
          <div className="grid flex-1 gap-3 overflow-y-auto pr-2 pl-1 py-1">
          {items.length ? (
            items.map((item, index) => {
              const isSelected = index === selectedIndex;
              return (
                <button
                  className={`group w-full rounded-3xl border px-4 py-4 text-left transition-all duration-300 ${
                    isSelected
                      ? "scale-[1.01] border-accent/40 bg-white shadow-[0_4px_20px_-4px_rgba(217,119,6,0.15)] ring-1 ring-accent/30"
                      : "border-slate-200/60 bg-white/60 hover:border-slate-300 hover:bg-white"
                  }`}
                  key={item.id}
                  onClick={() => setSelectedIndex(index)}
                  onDoubleClick={() => {
                    void confirmSelection(index);
                  }}
                  type="button"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div className="flex-1">
                      <div className="flex items-center gap-2">
                        <div className="flex h-5 w-5 shrink-0 items-center justify-center rounded-md bg-slate-100/80 text-[10px] font-bold text-slate-400 group-hover:bg-slate-200 group-hover:text-slate-500 transition-colors">
                          {index + 1}
                        </div>
                      </div>
                      <p className="mt-2.5 line-clamp-2 text-sm leading-relaxed text-slate-800">{item.contentPreview}</p>
                    </div>
                    {item.isFavorited ? (
                      <span className="shrink-0 rounded-full bg-amber-100/80 px-2 py-1 text-[10px] font-semibold text-amber-700">
                        收藏
                      </span>
                    ) : null}
                  </div>
                  <div className="mt-3.5 flex flex-wrap gap-x-4 gap-y-2 text-[11px] font-medium text-slate-400">
                    <span className="flex items-center gap-1.5"><span className="h-1 w-1 rounded-full bg-slate-300"></span>{item.sourceApp ?? "未知来源"}</span>
                    <span className="flex items-center gap-1.5"><span className="h-1 w-1 rounded-full bg-slate-300"></span>{formatDateTime(item.lastUsedAt ?? item.createdAt)}</span>
                  </div>
                </button>
              );
            })
          ) : (
            <div className="flex flex-col items-center justify-center rounded-3xl border border-dashed border-slate-300 bg-white/50 px-6 py-12 text-center transition-colors hover:border-slate-400 hover:bg-white/80">
              <div className="mb-4 flex h-10 w-10 items-center justify-center rounded-full bg-slate-100 ring-4 ring-white">
                <div className="h-3 w-3 rounded-sm bg-slate-300" />
              </div>
              <h3 className="font-display text-base font-medium text-slate-800">暂无历史记录</h3>
              <p className="mt-1.5 max-w-sm text-xs leading-relaxed text-slate-500">先复制一段文本，再按全局快捷键唤起速贴面板。</p>
            </div>
          )}
          </div>
        </div>

        <div className="mt-5 shrink-0 flex flex-col gap-3 rounded-2xl bg-slate-900/95 px-5 py-3.5 text-[11px] font-medium tracking-wide text-white/70 shadow-inner backdrop-blur sm:flex-row sm:items-center sm:justify-between">
          <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
            <span className="flex items-center gap-1.5"><kbd className="rounded-md border border-white/20 bg-white/10 px-1.5 py-0.5 font-sans text-white">↑</kbd><kbd className="rounded-md border border-white/20 bg-white/10 px-1.5 py-0.5 font-sans text-white">↓</kbd> 选择</span>
            <span className="flex items-center gap-1.5"><kbd className="rounded-md border border-white/20 bg-white/10 px-1.5 py-0.5 font-sans text-white">1</kbd> - <kbd className="rounded-md border border-white/20 bg-white/10 px-1.5 py-0.5 font-sans text-white">9</kbd> 直选</span>
            <span className="flex items-center gap-1.5"><kbd className="rounded-md border border-white/20 bg-white/10 px-1.5 py-0.5 font-sans text-white">↵</kbd> 粘贴</span>
            <span className="flex items-center gap-1.5"><kbd className="rounded-md border border-white/20 bg-white/10 px-1.5 py-0.5 font-sans text-white">Esc</kbd> 关闭</span>
          </div>
          {lastMessage ? <span className="text-amber-400">{lastMessage}</span> : <span className="hidden text-white/40 sm:inline-block">右上角进入资料库</span>}
        </div>
      </div>
    </div>
  );
}
