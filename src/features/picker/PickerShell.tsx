import { useEffect, useMemo, useRef, useState } from "react";
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
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

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
    const currentItem = itemRefs.current[selectedIndex];
    if (currentItem) {
      currentItem.scrollIntoView({
        behavior: "smooth",
        block: "nearest",
      });
    }
  }, [selectedIndex]);

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
    <div className="flex h-screen w-screen items-start justify-center bg-transparent p-0 text-ink overflow-hidden">
      <div className="flex h-full w-full flex-col overflow-hidden rounded-[20px] border border-white/40 bg-white/85 shadow-[0_16px_40px_rgba(15,23,42,0.2)] backdrop-blur-xl">
        <div className="flex shrink-0 items-center justify-between border-b border-slate-200/50 bg-white/40 px-4 py-2.5">
          <div className="flex items-center gap-2.5">
            <div className="h-2.5 w-2.5 rounded-full bg-accent/80"></div>
            <span className="text-xs font-semibold tracking-wide text-slate-700">FloatPaste</span>
          </div>
          <button
            className="flex items-center gap-1 rounded-full px-2 py-1 text-[10px] font-medium text-slate-500 transition-colors hover:bg-slate-200/50 hover:text-slate-800"
            onClick={() => void openManagerFromPicker()}
            type="button"
          >
            资料库
            <svg className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth={2} viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" d="M13.5 4.5 21 12m0 0-7.5 7.5M21 12H3" /></svg>
          </button>
        </div>

        <div className="flex min-h-0 flex-1 flex-col px-2 py-2">
          <div className="grid flex-1 gap-1 overflow-y-auto pr-1">
          {items.length ? (
            items.map((item, index) => {
              const isSelected = index === selectedIndex;
              return (
                <button
                  ref={(el) => {
                    itemRefs.current[index] = el;
                  }}
                  className={`group relative flex w-full items-start gap-2.5 rounded-xl px-2.5 py-2.5 text-left transition-all duration-150 ${
                    isSelected
                      ? "bg-amber-50 shadow-sm ring-1 ring-inset ring-amber-500/30"
                      : "hover:bg-slate-100/50"
                  }`}
                  key={item.id}
                  onClick={() => setSelectedIndex(index)}
                  onDoubleClick={() => {
                    void confirmSelection(index);
                  }}
                  type="button"
                >
                  <div className="mt-0.5 flex shrink-0 items-center justify-center">
                    <kbd className={`flex h-5 w-5 items-center justify-center rounded-md border-b-2 font-mono text-[10px] font-bold ${
                      isSelected 
                        ? "border-amber-600/40 bg-amber-500 text-white" 
                        : "border-slate-300 bg-slate-100 text-slate-500 group-hover:border-slate-400 group-hover:bg-slate-200 group-hover:text-slate-700"
                    }`}>
                      {index + 1}
                    </kbd>
                  </div>
                  
                  <div className="flex-1 min-w-0">
                    <p className={`line-clamp-2 text-xs leading-relaxed ${isSelected ? "font-medium text-amber-950" : "text-slate-700"}`}>
                      {item.contentPreview}
                    </p>
                    <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[10px] font-medium text-slate-400">
                      <span className="truncate max-w-[100px]">{item.sourceApp ?? "未知来源"}</span>
                      <span className="w-0.5 h-0.5 rounded-full bg-slate-300"></span>
                      <span>{formatDateTime(item.lastUsedAt ?? item.createdAt)}</span>
                      {item.isFavorited ? (
                        <>
                           <span className="w-0.5 h-0.5 rounded-full bg-slate-300"></span>
                           <span className="text-amber-600">★</span>
                        </>
                      ) : null}
                    </div>
                  </div>
                </button>
              );
            })
          ) : (
            <div className="flex flex-col items-center justify-center py-8 text-center">
              <div className="mb-2.5 flex h-6 w-6 items-center justify-center rounded-full bg-slate-100 ring-4 ring-white">
                <div className="h-1.5 w-1.5 rounded-sm bg-slate-300" />
              </div>
              <p className="text-xs font-medium text-slate-600">暂无历史记录</p>
            </div>
          )}
          </div>
        </div>

        <div className="flex shrink-0 items-center justify-between border-t border-slate-200/50 bg-white/60 px-4 py-2 text-[10px] font-medium tracking-wide text-slate-500 backdrop-blur-md">
          <div className="flex items-center gap-3">
            <span className="flex items-center gap-1"><kbd className="font-sans font-bold">↑↓</kbd> 导航</span>
            <span className="flex items-center gap-1"><kbd className="font-sans font-bold">↵</kbd> 粘贴</span>
            <span className="flex items-center gap-1"><kbd className="font-sans font-bold">Esc</kbd> 取消</span>
          </div>
          {lastMessage ? <span className="text-amber-600 font-semibold">{lastMessage}</span> : null}
        </div>
      </div>
    </div>
  );
}
