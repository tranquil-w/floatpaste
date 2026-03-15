import { type MouseEvent as ReactMouseEvent, useEffect, useMemo, useRef, useState } from "react";
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
import { startCurrentWindowResize, type WindowResizeDirection } from "../../bridge/window";
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

const PICKER_RESIZE_HANDLES: Array<{
  direction: WindowResizeDirection;
  className: string;
}> = [
    {
      direction: "North",
      className: "absolute inset-x-3 top-0 z-20 h-2 cursor-ns-resize",
    },
    {
      direction: "South",
      className: "absolute inset-x-3 bottom-0 z-20 h-2 cursor-ns-resize",
    },
    {
      direction: "West",
      className: "absolute inset-y-3 left-0 z-20 w-2 cursor-ew-resize",
    },
    {
      direction: "East",
      className: "absolute inset-y-3 right-0 z-20 w-2 cursor-ew-resize",
    },
    {
      direction: "NorthWest",
      className: "absolute left-0 top-0 z-30 h-4 w-4 cursor-nwse-resize",
    },
    {
      direction: "NorthEast",
      className: "absolute right-0 top-0 z-30 h-4 w-4 cursor-nesw-resize",
    },
    {
      direction: "SouthWest",
      className: "absolute bottom-0 left-0 z-30 h-4 w-4 cursor-nesw-resize",
    },
    {
      direction: "SouthEast",
      className: "absolute bottom-0 right-0 z-30 h-4 w-4 cursor-nwse-resize",
    },
  ];

export function PickerShell() {
  const tauriRuntime = isTauriRuntime();
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

  const handleResizeMouseDown =
    (direction: WindowResizeDirection) =>
      (event: ReactMouseEvent<HTMLDivElement>) => {
        event.preventDefault();
        event.stopPropagation();
        void startCurrentWindowResize(direction).catch((error) => {
          console.warn("启动 picker 窗口拉伸失败", error);
        });
      };

  const confirmSelection = async (index: number) => {
    const item = itemsRef.current[index];
    if (!item) {
      return;
    }

    const result = await pasteItem(item.id, { restoreClipboardAfterPaste: true, pasteToTarget: true });
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
    if (!tauriRuntime) {
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
  }, [tauriRuntime]);

  useEffect(() => {
    if (tauriRuntime) {
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
  }, [tauriRuntime]);

  return (
    <div className="flex h-screen w-screen items-start justify-center overflow-hidden bg-transparent p-0 text-ink select-none">
      <div className="relative flex h-full w-full flex-col overflow-hidden rounded-[18px] border border-[color:var(--cp-border-soft)] bg-[color:var(--cp-window-shell)]/98 shadow-[0_12px_40px_-8px_rgba(0,0,0,0.3)] ring-1 ring-black/5 ring-inset backdrop-blur-xl dark:border-[color:var(--cp-border-soft)]/80 dark:bg-[color:var(--cp-window-shell)]/98 dark:shadow-[0_20px_50px_-12px_rgba(0,0,0,0.8)] dark:ring-[color:var(--cp-border-soft)]/20">
        {tauriRuntime
          ? PICKER_RESIZE_HANDLES.map((handle) => (
            <div
              aria-hidden="true"
              className={handle.className}
              key={handle.direction}
              onMouseDown={handleResizeMouseDown(handle.direction)}
            />
          ))
          : null}
        <div className="flex shrink-0 items-center justify-between border-b border-[color:var(--cp-border-soft)]/60 bg-[color:var(--cp-panel-surface)]/40 px-3 py-2 dark:border-[color:var(--cp-border-soft)]/70 dark:bg-[color:var(--cp-panel-surface)]/70" data-tauri-drag-region>
          <div className="flex items-center gap-2">
            <div className="h-2.5 w-2.5 rounded-full bg-[color:var(--cp-accent-primary)] shadow-sm"></div>
            <span className="text-[12px] font-bold tracking-tight text-[color:var(--cp-text-primary)]">FloatPaste</span>
            {lastMessage && <span className="ml-2 animate-pulse text-[10px] font-medium text-[color:var(--cp-accent-warm)]">{lastMessage}</span>}
          </div>
          <div className="flex items-center gap-1.5">
            <button
              className="flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-semibold text-[color:var(--cp-text-secondary)] transition-colors hover:bg-[color:var(--cp-control-surface-hover)]/50 hover:text-[color:var(--cp-text-primary)]"
              onClick={() => void handleOpenManager()}
              type="button"
            >
              资料库
              <svg className="h-3 w-3" fill="none" stroke="currentColor" strokeWidth={2.2} viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" d="M13.5 4.5 21 12m0 0-7.5 7.5M21 12H3" /></svg>
            </button>
          </div>
        </div>

        <div className="flex min-h-0 flex-1 flex-col p-2">
          <div className="grid flex-1 gap-1 overflow-y-auto overflow-x-hidden px-1.5 pt-1 pb-1 transition-colors">
            {items.map((item, index) => {
              const isSelected = index === selectedIndex;
              return (
                <button
                  ref={(el) => {
                    itemRefs.current[index] = el;
                  }}
                  className={`group relative flex w-full flex-col gap-1.5 rounded-[12px] px-3 py-2.5 text-left transition-all duration-200 ${isSelected
                    ? "bg-[color:var(--cp-accent-primary)]/15 shadow-sm ring-1 ring-[color:var(--cp-accent-primary)]/30 dark:bg-[color:var(--cp-accent-primary)]/20 dark:shadow-none dark:ring-[color:var(--cp-accent-primary)]/40"
                    : "bg-transparent hover:bg-[color:var(--cp-control-surface-hover)]/30 dark:hover:bg-[color:var(--cp-control-surface-hover)]/40"
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
                  <p
                    className={`${isSelected ? "text-[color:var(--cp-text-primary)]" : "text-[color:var(--cp-text-primary)]/90 dark:text-[color:var(--cp-text-primary)]/80"} line-clamp-5 text-[13px] font-medium leading-[1.6] tracking-tight break-words [overflow-wrap:anywhere] whitespace-pre-wrap transition-colors`}
                    title={item.tooltipText || item.contentPreview}
                  >
                    {item.contentPreview}
                  </p>

                  <div className={`flex w-full items-center gap-2 text-[10px] leading-none transition-colors ${isSelected ? "text-[color:var(--cp-accent-primary)]/80" : "text-[color:var(--cp-text-muted)]"}`}>
                    {index < 9 ? (
                      <kbd className={`flex h-[16px] min-w-[16px] px-1 items-center justify-center rounded-[5px] font-mono text-[9px] font-bold transition-colors ${isSelected
                        ? "bg-[color:var(--cp-accent-primary)] text-cp-base dark:text-cp-mantle"
                        : "bg-[color:var(--cp-control-surface)] text-[color:var(--cp-text-secondary)] group-hover:bg-[color:var(--cp-control-surface-hover)] group-hover:text-[color:var(--cp-text-primary)]"
                        }`}>
                        {index + 1}
                      </kbd>
                    ) : null}
                    <span className="shrink-0 rounded-[4px] bg-[color:var(--cp-control-surface)]/60 px-1.5 py-0.5 font-medium text-[color:var(--cp-text-secondary)]">
                      {getClipTypeLabel(item)}
                    </span>
                    <span className="min-w-0 flex-1 truncate font-medium opacity-80">
                      {item.sourceApp ?? "未知来源"}
                    </span>
                    <span className="flex shrink-0 items-center gap-1 font-medium opacity-80">
                      <span className="tabular-nums">
                        {formatDateTime(item.lastUsedAt ?? item.createdAt)}
                      </span>
                      {item.isFavorited ? (
                        <span className="text-[10px] text-[color:var(--cp-accent-warm)]">★</span>
                      ) : null}
                    </span>
                  </div>
                </button>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}
