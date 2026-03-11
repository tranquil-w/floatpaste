import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { queryClient } from "../../app/queryClient";
import { hidePicker, openManagerFromPicker, pasteItem } from "../../bridge/commands";
import {
  CLIPS_CHANGED_EVENT,
  PICKER_CONFIRM_EVENT,
  PICKER_NAVIGATE_EVENT,
  PICKER_SELECT_INDEX_EVENT,
  SETTINGS_CHANGED_EVENT,
  PICKER_SESSION_START_EVENT,
} from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import type { ClipItemSummary } from "../../shared/types/clips";
import { getClipTypeLabel } from "../../shared/utils/clipDisplay";
import { formatDateTime } from "../../shared/utils/time";
import {
  DEFAULT_PICKER_RECORD_LIMIT,
  normalizePickerRecordLimit,
  usePickerRecentQuery,
  usePickerSettingsQuery,
} from "./queries";

export function PickerShell() {
  const settings = usePickerSettingsQuery();
  const pickerRecordLimit = settings.data
    ? normalizePickerRecordLimit(settings.data.pickerRecordLimit)
    : DEFAULT_PICKER_RECORD_LIMIT;
  const recent = usePickerRecentQuery(pickerRecordLimit, Boolean(settings.data));
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [lastMessage, setLastMessage] = useState("");
  const itemsRef = useRef<ClipItemSummary[]>([]);
  const selectedIndexRef = useRef(0);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);

  const items = useMemo(() => recent.data ?? [], [recent.data]);

  const handleOpenManager = async () => {
    await openManagerFromPicker();
  };

  const confirmSelection = async (index: number) => {
    const item = itemsRef.current[index];
    if (!item) {
      return;
    }

    const result = await pasteItem(item.id, { restoreClipboardAfterPaste: true });
    setLastMessage(result.message);
  };

  useEffect(() => {
    itemsRef.current = items;
  }, [items]);

  useEffect(() => {
    selectedIndexRef.current = selectedIndex;
  }, [selectedIndex]);

  useEffect(() => {
    if (selectedIndex >= items.length) {
      selectedIndexRef.current = 0;
      setSelectedIndex(0);
    }
  }, [items.length, selectedIndex]);

  useEffect(() => {
    const currentItem = itemRefs.current[selectedIndex];
    if (currentItem) {
      currentItem.scrollIntoView({
        behavior: "auto",
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
    let unlistenSettings: (() => void) | undefined;
    let unlistenNavigate: (() => void) | undefined;
    let unlistenConfirm: (() => void) | undefined;
    let unlistenSelectIndex: (() => void) | undefined;

    void listen(PICKER_SESSION_START_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      await queryClient.invalidateQueries({ queryKey: ["picker-recent"] });

      if (!disposed) {
        selectedIndexRef.current = 0;
        setSelectedIndex(0);
        setLastMessage("");
      }
    }).then((cleanup) => {
      unlistenStart = cleanup;
    });

    void listen(CLIPS_CHANGED_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["picker-recent"] });
    }).then((cleanup) => {
      unlistenClips = cleanup;
    });

    void listen(SETTINGS_CHANGED_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      await queryClient.invalidateQueries({ queryKey: ["picker-recent"] });

      if (!disposed) {
        selectedIndexRef.current = 0;
        setSelectedIndex(0);
      }
    }).then((cleanup) => {
      unlistenSettings = cleanup;
    });

    void listen<string>(PICKER_NAVIGATE_EVENT, async (event) => {
      const itemCount = itemsRef.current.length;
      if (disposed || !itemCount) {
        return;
      }

      setSelectedIndex((current) => {
        const nextIndex =
          event.payload === "up"
            ? (current - 1 + itemCount) % itemCount
            : (current + 1) % itemCount;
        selectedIndexRef.current = nextIndex;
        return nextIndex;
      });
    }).then((cleanup) => {
      unlistenNavigate = cleanup;
    });

    void listen(PICKER_CONFIRM_EVENT, async () => {
      if (disposed) {
        return;
      }

      await confirmSelection(selectedIndexRef.current);
    }).then((cleanup) => {
      unlistenConfirm = cleanup;
    });

    void listen<number>(PICKER_SELECT_INDEX_EVENT, async (event) => {
      const itemCount = itemsRef.current.length;
      if (disposed || !itemCount) {
        return;
      }

      const index = Math.max(0, Math.min(event.payload, itemCount - 1));
      selectedIndexRef.current = index;
      setSelectedIndex(index);
      await confirmSelection(index);
    }).then((cleanup) => {
      unlistenSelectIndex = cleanup;
    });

    return () => {
      disposed = true;
      unlistenStart?.();
      unlistenClips?.();
      unlistenSettings?.();
      unlistenNavigate?.();
      unlistenConfirm?.();
      unlistenSelectIndex?.();
    };
  }, []);

  useEffect(() => {
    if (isTauriRuntime()) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      const itemCount = itemsRef.current.length;
      if (!itemCount && event.key !== "Escape") {
        return;
      }

      if (event.key === "Escape") {
        event.preventDefault();
        void hidePicker();
        return;
      }

      if (event.key === "ArrowUp") {
        event.preventDefault();
        setSelectedIndex((current) => {
          const nextIndex = (current - 1 + itemCount) % itemCount;
          selectedIndexRef.current = nextIndex;
          return nextIndex;
        });
        return;
      }

      if (event.key === "ArrowDown") {
        event.preventDefault();
        setSelectedIndex((current) => {
          const nextIndex = (current + 1) % itemCount;
          selectedIndexRef.current = nextIndex;
          return nextIndex;
        });
        return;
      }

      if (event.key === "Enter") {
        event.preventDefault();
        void confirmSelection(selectedIndexRef.current);
        return;
      }

      if (event.key === "Tab") {
        event.preventDefault();
        void handleOpenManager();
        return;
      }

      if (/^[1-9]$/.test(event.key)) {
        event.preventDefault();
        const index = Math.min(Number(event.key) - 1, itemCount - 1);
        if (index >= 0) {
          selectedIndexRef.current = index;
          setSelectedIndex(index);
          void confirmSelection(index);
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  return (
    <div className="flex h-screen w-screen items-start justify-center bg-transparent p-0 text-ink overflow-hidden select-none" data-tauri-drag-region>
      <div className="flex h-full w-full flex-col overflow-hidden rounded-[20px] border border-slate-300/50 bg-white/95 backdrop-blur-xl">
        <div className="flex shrink-0 items-center justify-between border-b border-slate-200/50 bg-white/70 px-4 py-3" data-tauri-drag-region>
          <div className="flex items-center gap-2.5">
            <div className="h-2.5 w-2.5 rounded-full bg-amber-400"></div>
            <span className="text-[13px] font-semibold tracking-wide text-slate-700">FloatPaste</span>
          </div>
          <div className="flex items-center gap-2">
            <button
              className="flex items-center gap-1 rounded-full px-2.5 py-1 text-[11px] font-semibold text-slate-500 transition-colors hover:bg-slate-200/50 hover:text-slate-800"
              onClick={() => void handleOpenManager()}
              type="button"
            >
              资料库
              <svg className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth={2} viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" d="M13.5 4.5 21 12m0 0-7.5 7.5M21 12H3" /></svg>
            </button>
          </div>
        </div>

        <div className="flex min-h-0 flex-1 flex-col px-3 py-3">
          <div className="grid flex-1 gap-1.5 overflow-y-auto overflow-x-hidden pr-2 [&::-webkit-scrollbar]:w-1.5 [&::-webkit-scrollbar-thumb]:rounded-full [&::-webkit-scrollbar-thumb]:bg-slate-300/80 hover:[&::-webkit-scrollbar-thumb]:bg-slate-400/80 [&::-webkit-scrollbar-track]:bg-transparent transition-colors">
            {items.length ? (
              items.map((item, index) => {
                const isSelected = index === selectedIndex;
                return (
                  <button
                    ref={(el) => {
                      itemRefs.current[index] = el;
                    }}
                    className={`group relative flex w-full items-start gap-3 rounded-xl px-3 py-3 text-left transition-all duration-200 ${isSelected
                      ? "bg-amber-500/10 shadow-[0_2px_10px_rgba(245,158,11,0.05)]"
                      : "bg-transparent hover:bg-slate-500/5"
                      }`}
                    key={item.id}
                    onClick={() => {
                      selectedIndexRef.current = index;
                      setSelectedIndex(index);
                    }}
                    onDoubleClick={() => {
                      void confirmSelection(index);
                    }}
                    type="button"
                  >
                    <div className="mt-0.5 flex shrink-0 items-center justify-center">
                      {index < 9 ? (
                        <kbd className={`flex h-[20px] w-[20px] items-center justify-center rounded-[6px] font-mono text-[11px] font-bold transition-colors ${isSelected
                          ? "bg-amber-500 text-white shadow-sm"
                          : "bg-slate-400/15 text-slate-400 group-hover:bg-slate-400/25 group-hover:text-slate-700"
                          }`}>
                          {index + 1}
                        </kbd>
                      ) : (
                        <span aria-hidden="true" className="h-[20px] w-[20px]" />
                      )}
                    </div>

                    <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                      <div className="flex items-center gap-2 mb-0.5">
                        <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-slate-100 text-slate-500 font-medium">
                          {getClipTypeLabel(item)}
                        </span>
                      </div>
                      <p
                        className={`${isSelected ? "text-slate-800 font-medium" : "text-slate-600"} line-clamp-2 text-[13px] leading-[1.6] break-words [overflow-wrap:anywhere] transition-colors`}
                      >
                        {item.contentPreview}
                      </p>
                      <div className={`flex min-w-0 items-center justify-between gap-2 text-[11px] leading-none transition-colors ${isSelected ? "text-amber-700/60" : "text-slate-400/70"}`}>
                        <span className="min-w-0 flex-1 truncate font-medium">
                          {item.sourceApp ?? "未知来源"}
                        </span>
                        <span className="flex shrink-0 items-center gap-1.5 font-medium">
                          <span className="tabular-nums">
                            {formatDateTime(item.lastUsedAt ?? item.createdAt)}
                          </span>
                          {item.isFavorited ? (
                            <span className="text-[10px] leading-none text-amber-500">★</span>
                          ) : null}
                        </span>
                      </div>
                    </div>
                  </button>
                );
              })
            ) : (
              <div className="flex flex-col items-center justify-center py-8 text-center">
                <div className="mb-2.5 flex h-6 w-6 items-center justify-center rounded-full bg-slate-100 ring-4 ring-white">
                  <div className="h-1.5 w-1.5 rounded-sm bg-slate-400" />
                </div>
                <p className="text-[13px] font-medium text-slate-500">暂无历史记录</p>
              </div>
            )}
          </div>
        </div>

        <div className="flex shrink-0 items-center justify-between border-t border-slate-200/50 bg-slate-50/80 px-4 py-2.5 text-[11px] font-medium tracking-wide text-slate-400 backdrop-blur-md">
          <div className="flex items-center gap-3.5">
            <span className="flex items-center gap-1.5"><kbd className="flex h-[18px] items-center justify-center rounded bg-slate-400/15 px-1.5 font-sans text-[10px] font-bold text-slate-500">↑↓</kbd> 导航</span>
            <span className="flex items-center gap-1.5"><kbd className="flex h-[18px] items-center justify-center rounded bg-slate-400/15 px-1.5 font-sans text-[10px] font-bold text-slate-500">↵</kbd> 粘贴</span>
            <span className="flex items-center gap-1.5"><kbd className="flex h-[18px] items-center justify-center rounded bg-slate-400/15 px-1.5 font-sans text-[10px] font-bold text-slate-500">Esc</kbd> 取消</span>
          </div>
          <div className="flex items-center gap-3">
            {lastMessage ? <span className="font-semibold text-amber-600">{lastMessage}</span> : null}
          </div>
        </div>
      </div>
    </div>
  );
}
