import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { queryClient } from "../../app/queryClient";
import { hidePicker, hideTooltip, openEditorFromPicker, pasteItem, setItemFavorited, showTooltip } from "../../bridge/commands";
import {
  CLIPS_CHANGED_EVENT,
  PICKER_CONFIRM_EVENT,
  PICKER_FAVORITE_EVENT,
  PICKER_NAVIGATE_EVENT,
  PICKER_OPEN_EDITOR_EVENT,
  PICKER_SELECT_INDEX_EVENT,
  PICKER_SESSION_END_EVENT,
  PICKER_SESSION_START_EVENT,
  SETTINGS_CHANGED_EVENT,
} from "../../bridge/events";
import { getImageUrl } from "../../bridge/imageUrl";
import { isTauriRuntime } from "../../bridge/runtime";
import type { ClipItemSummary } from "../../shared/types/clips";
import { getClipTypeLabel } from "../../shared/utils/clipDisplay";
import { formatDateTime } from "../../shared/utils/time";
import { LoadingSpinner } from "../../shared/ui/LoadingSpinner";
import {
  WindowResizeHandles,
  type WindowResizeHandle,
} from "../../shared/ui/WindowResizeHandles";
import {
  DEFAULT_PICKER_RECORD_LIMIT,
  normalizePickerRecordLimit,
  usePickerRecentQuery,
  usePickerSettingsQuery,
} from "./queries";
import { toggleFavoriteSelection } from "./favoriteToggle";
import { PICKER_IMAGE_THUMBNAIL_STYLE } from "./previewLayout";
import { buildTooltipHtml } from "./tooltipHtml";
import { resolveTooltipShowPosition } from "./tooltipState";

const STYLES = {
  container:
    "flex h-screen w-screen flex-col overflow-hidden rounded-md border border-pg-border-muted bg-pg-canvas-default",
  header:
    "flex shrink-0 items-center justify-between border-b border-pg-border-subtle bg-pg-canvas-default px-3 py-1.5",
  headerDot: "h-2 w-2 rounded-full bg-pg-accent-fg shadow-[0_0_0_3px_rgba(var(--pg-blue-5-rgb),0.10)]",
  headerMessage:
    "ml-2 rounded-[3px] bg-pg-accent-subtle px-1.5 py-0.5 text-[10px] font-medium leading-none text-pg-accent-fg",
  headerButton:
    "flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[11px] font-semibold text-pg-fg-muted transition-colors hover:bg-pg-accent-subtle hover:text-pg-fg-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-pg-accent-fg focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-45",
  itemButton: (selected: boolean, favorited: boolean) => `group relative flex w-full flex-col gap-1.5 rounded-[8px] px-2.5 py-2 text-left transition-colors border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-pg-accent-fg focus-visible:ring-offset-2 ${
    selected
      ? "border-[color:rgba(var(--pg-blue-5-rgb),0.35)] bg-pg-accent-subtle shadow-[0_1px_0_rgba(var(--pg-shadow-color),0.14),inset_0_0_0_1px_rgba(var(--pg-blue-5-rgb),0.08)]"
      : favorited
        ? "border-pg-border-subtle border-l-[3px] border-l-pg-accent-fg bg-pg-canvas-default hover:border-pg-border-default hover:bg-pg-canvas-subtle"
        : "bg-pg-canvas-default border-pg-border-subtle hover:border-pg-border-default hover:bg-pg-canvas-subtle"
  }`,
  itemContent: (selected: boolean, favorited: boolean) =>
    `${selected ? "text-pg-fg-default font-semibold" : favorited ? "text-pg-fg-default font-medium" : "text-pg-fg-muted font-medium"} line-clamp-4 text-[13px] leading-[1.55] tracking-tight break-words [overflow-wrap:anywhere] whitespace-pre-wrap transition-colors`,
  kbdBadge: (selected: boolean) => `inline-flex h-[18px] min-w-[18px] px-1.5 items-center justify-center rounded-[4px] font-mono text-[9px] font-bold transition-colors ${
    selected
      ? "bg-pg-accent-fg text-pg-fg-on-emphasis shadow-[inset_0_0_0_1px_rgba(255,255,255,0.10)]"
      : "bg-pg-canvas-subtle text-pg-fg-subtle group-hover:bg-pg-neutral-3 group-hover:text-pg-fg-muted"
  }`,
  typeBadge: (selected: boolean) =>
    `shrink-0 rounded-[3px] px-1.5 py-0.5 text-[10px] font-medium ${
      selected
        ? "bg-pg-canvas-default text-pg-fg-muted"
        : "bg-pg-neutral-3 text-pg-fg-subtle"
    }`,
};

const PICKER_RESIZE_HANDLES: WindowResizeHandle[] = [
  {
    key: "north-left",
    direction: "North",
    className:
      "absolute left-3 right-[calc(50%+2.25rem)] top-0 z-20 h-2 cursor-ns-resize",
  },
  {
    key: "north-right",
    direction: "North",
    className:
      "absolute left-[calc(50%+2.25rem)] right-3 top-0 z-20 h-2 cursor-ns-resize",
  },
  {
    key: "south",
    direction: "South",
    className: "absolute inset-x-3 bottom-0 z-20 h-2 cursor-ns-resize",
  },
  {
    key: "west",
    direction: "West",
    className: "absolute inset-y-3 left-0 z-20 w-2 cursor-ew-resize",
  },
  {
    key: "east",
    direction: "East",
    className: "absolute inset-y-3 right-0 z-20 w-2 cursor-ew-resize",
  },
  {
    key: "north-west",
    direction: "NorthWest",
    className: "absolute left-0 top-0 z-30 h-4 w-4 cursor-nwse-resize",
  },
  {
    key: "north-east",
    direction: "NorthEast",
    className: "absolute right-0 top-0 z-30 h-4 w-4 cursor-nesw-resize",
  },
  {
    key: "south-west",
    direction: "SouthWest",
    className: "absolute bottom-0 left-0 z-30 h-4 w-4 cursor-nesw-resize",
  },
  {
    key: "south-east",
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
  const favoriteTogglePendingRef = useRef(false);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const tooltipTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const tooltipRequestIdRef = useRef(0);
  const imageUrlCacheRef = useRef(new Map<string, string | null>());
  const imageUrlPendingRef = useRef(new Set<string>());
  const [, setImageUrlVersion] = useState(0);

  const items = useMemo(() => recent.data ?? [], [recent.data]);
  const selectedItem = items[selectedIndex] ?? null;
  const canEditSelected = selectedItem?.type === "text";

  const clearTooltipTimer = () => {
    if (tooltipTimerRef.current) {
      clearTimeout(tooltipTimerRef.current);
      tooltipTimerRef.current = null;
    }
  };

  const invalidateTooltipRequest = () => {
    tooltipRequestIdRef.current += 1;
    return tooltipRequestIdRef.current;
  };

  const cancelTooltip = () => {
    clearTooltipTimer();
    invalidateTooltipRequest();

    if (tauriRuntime) {
      void hideTooltip();
    }
  };

  const confirmSelection = async (index: number) => {
    const item = itemsRef.current[index];
    if (!item) {
      return;
    }

    cancelTooltip();

    await pasteItem(item.id, {
      restoreClipboardAfterPaste: settings.data?.restoreClipboardAfterPaste ?? true,
      pasteToTarget: true,
    });
    // picker 紧接着会被隐藏，不清空的话下次打开时会闪过旧消息
    setLastMessage("");
  };

  const handleOpenEditor = async () => {
    const item = itemsRef.current[selectedIndexRef.current];
    if (!item) {
      return;
    }

    if (item.type !== "text") {
      setLastMessage("当前仅文本条目支持编辑");
      return;
    }

    cancelTooltip();
    await openEditorFromPicker(item.id);
  };

  const handleToggleFavorite = async () => {
    await toggleFavoriteSelection({
      item: itemsRef.current[selectedIndexRef.current],
      isPending: () => favoriteTogglePendingRef.current,
      setPending: (pending) => {
        favoriteTogglePendingRef.current = pending;
      },
      setItemFavorited,
      refreshItems: async () => {
        await queryClient.invalidateQueries({ queryKey: ["picker-recent"] });
      },
      setLastMessage,
      onError: (error) => {
        console.error("更新收藏状态失败", error);
      },
    });
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
    let disposed = false;
    const imageItems = items.filter((item) =>
      item.type === "image"
      && Boolean(item.imagePath)
      && !imageUrlCacheRef.current.has(item.id)
      && !imageUrlPendingRef.current.has(item.id)
    );

    for (const item of imageItems) {
      imageUrlPendingRef.current.add(item.id);
      void getImageUrl(item.imagePath).then((imageUrl) => {
        imageUrlCacheRef.current.set(item.id, imageUrl);
      }).catch(() => {
        imageUrlCacheRef.current.set(item.id, null);
      }).finally(() => {
        imageUrlPendingRef.current.delete(item.id);
        if (!disposed) {
          setImageUrlVersion((current) => current + 1);
        }
      });
    }

    return () => {
      disposed = true;
    };
  }, [items]);

  useEffect(() => {
    if (!tauriRuntime) {
      return;
    }

    let disposed = false;
    let unlistenStart: (() => void) | undefined;
    let unlistenEnd: (() => void) | undefined;
    let unlistenClips: (() => void) | undefined;
    let unlistenSettings: (() => void) | undefined;
    let unlistenNavigate: (() => void) | undefined;
    let unlistenConfirm: (() => void) | undefined;
    let unlistenSelectIndex: (() => void) | undefined;
    let unlistenOpenEditor: (() => void) | undefined;
    let unlistenFavorite: (() => void) | undefined;

    void listen(PICKER_SESSION_END_EVENT, () => {
      if (!disposed) {
        cancelTooltip();
        selectedIndexRef.current = 0;
        setSelectedIndex(0);
      }
    }).then((cleanup) => {
      unlistenEnd = cleanup;
    });

    void listen(PICKER_SESSION_START_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      await queryClient.invalidateQueries({ queryKey: ["picker-recent"] });

      if (!disposed) {
        cancelTooltip();
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

    void listen<string>(PICKER_NAVIGATE_EVENT, (event) => {
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

    void listen(PICKER_OPEN_EDITOR_EVENT, async () => {
      if (disposed) {
        return;
      }
      await handleOpenEditor();
    }).then((cleanup) => {
      unlistenOpenEditor = cleanup;
    });

    void listen(PICKER_FAVORITE_EVENT, async () => {
      if (disposed) {
        return;
      }
      await handleToggleFavorite();
    }).then((cleanup) => {
      unlistenFavorite = cleanup;
    });

    return () => {
      disposed = true;
      unlistenEnd?.();
      unlistenStart?.();
      unlistenClips?.();
      unlistenSettings?.();
      unlistenNavigate?.();
      unlistenConfirm?.();
      unlistenSelectIndex?.();
      unlistenOpenEditor?.();
      unlistenFavorite?.();
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
        cancelTooltip();
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

      if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
        event.preventDefault();
        void handleOpenEditor();
        return;
      }

      if (event.key === " ") {
        event.preventDefault();
        void handleToggleFavorite();
        return;
      }

      if (event.key === "Enter") {
        event.preventDefault();
        void confirmSelection(selectedIndexRef.current);
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

  const handleItemMouseMove = (event: React.MouseEvent, item: ClipItemSummary) => {
    if (!tauriRuntime) return;
    const requestId = invalidateTooltipRequest();
    const clientPosition = { x: event.clientX, y: event.clientY };
    clearTooltipTimer();
    tooltipTimerRef.current = setTimeout(() => {
      tooltipTimerRef.current = null;
      void (async () => {
        const imageUrl = await resolveItemImageUrl(item);
        if (tooltipRequestIdRef.current !== requestId) {
          return;
        }

        const tooltipHtml = buildTooltipHtml(item, { imageUrl, requestId });
        const currentWindow = getCurrentWebviewWindow();
        const [outerPosition, scaleFactor] = await Promise.all([
          currentWindow.outerPosition(),
          currentWindow.scaleFactor(),
        ]);
        const position = resolveTooltipShowPosition({
          activeRequestId: tooltipRequestIdRef.current,
          requestId,
          outerPosition,
          scaleFactor,
          clientPosition,
        });

        if (!position) {
          return;
        }

        await showTooltip(
          requestId,
          position.x,
          position.y,
          tooltipHtml,
          (document.documentElement.dataset.theme as "dark" | "light") ?? "dark",
        );
      })().catch((error) => {
        console.warn("[FloatPaste] tooltip 定位或显示失败:", error);
      });
    }, 100);
  };

  const handleItemMouseLeave = () => {
    if (!tauriRuntime) return;
    cancelTooltip();
  };

  const handleThumbnailError = (itemId: string) => {
    if (imageUrlCacheRef.current.get(itemId) === null) {
      return;
    }

    imageUrlCacheRef.current.set(itemId, null);
    setImageUrlVersion((current) => current + 1);
  };

  const resolveItemImageUrl = async (item: ClipItemSummary): Promise<string | null> => {
    if (item.type !== "image" || !item.imagePath) {
      return null;
    }

    const cachedImageUrl = imageUrlCacheRef.current.get(item.id);
    if (cachedImageUrl !== undefined) {
      return cachedImageUrl;
    }

    try {
      const imageUrl = await getImageUrl(item.imagePath);
      imageUrlCacheRef.current.set(item.id, imageUrl);
      setImageUrlVersion((current) => current + 1);
      return imageUrl;
    } catch {
      imageUrlCacheRef.current.set(item.id, null);
      setImageUrlVersion((current) => current + 1);
      return null;
    }
  };

  useEffect(() => {
    return () => {
      cancelTooltip();
    };
  }, []);

  return (
    <div className="m-0 h-screen w-screen select-none overflow-hidden bg-transparent p-0 text-pg-fg-default">
      <div className={STYLES.container}>
        {tauriRuntime
          ? (
            <WindowResizeHandles handles={PICKER_RESIZE_HANDLES} errorLabel="速贴" />
            )
          : null}

        <div className={STYLES.header}>
          <div className="flex min-w-0 flex-1 items-center gap-2" data-tauri-drag-region>
            <div aria-hidden="true" className={STYLES.headerDot} />
            <span className="text-[11px] font-semibold tracking-[0.02em] text-pg-fg-muted">
              FloatPaste
            </span>
            {lastMessage ? (
              <span className={STYLES.headerMessage}>
                {lastMessage}
              </span>
            ) : null}
          </div>
        </div>

        <div className="flex min-h-0 flex-1 flex-col rounded-b-md bg-pg-canvas-subtle px-1 py-1.5">
          {recent.isLoading ? (
            <div className="flex h-full items-center justify-center">
              <LoadingSpinner size="sm" text="正在加载记录..." />
            </div>
          ) : !recent.isLoading && items.length === 0 ? (
            <div className="flex flex-col items-center justify-center flex-1 gap-1 py-8">
              <p className="text-sm text-pg-fg-muted">暂无剪贴板记录</p>
              <p className="text-xs text-pg-fg-subtle">复制内容后按 Alt+Q 打开此面板</p>
            </div>
          ) : (
          <div className="grid flex-1 gap-1 overflow-y-auto overflow-x-hidden px-0.5 transition-colors">
            {items.map((item, index) => {
              const isSelected = index === selectedIndex;
              const imageUrl = item.type === "image"
                ? (imageUrlCacheRef.current.get(item.id) ?? null)
                : null;
              return (
                <button
                  ref={(el) => {
                    itemRefs.current[index] = el;
                  }}
                  className={STYLES.itemButton(isSelected, item.isFavorited)}
                  key={item.id}
                  onClick={() => {
                    selectedIndexRef.current = index;
                    setSelectedIndex(index);
                  }}
                  onDoubleClick={() => {
                    void confirmSelection(index);
                  }}
                  onMouseMove={(e) => handleItemMouseMove(e, item)}
                  onMouseLeave={handleItemMouseLeave}
                  type="button"
                >
                  <div className="flex items-start gap-2.5">
                    {imageUrl ? (
                      <img
                        alt=""
                        className={`mt-0.5 shrink-0 rounded-[6px] border object-contain ${
                          isSelected
                            ? "border-pg-border-default bg-pg-canvas-default"
                            : "border-pg-border-subtle bg-pg-canvas-subtle"
                        }`}
                        onError={() => handleThumbnailError(item.id)}
                        src={imageUrl}
                        style={PICKER_IMAGE_THUMBNAIL_STYLE}
                      />
                    ) : null}
                    <div className="min-w-0 flex-1">
                      <span
                        className={STYLES.itemContent(isSelected, item.isFavorited)}
                      >
                        {item.contentPreview}
                      </span>
                    </div>
                  </div>

                  <div
                    className={`flex w-full items-center gap-2 text-[10px] leading-none transition-colors ${
                      isSelected
                        ? "text-pg-fg-muted"
                        : "text-pg-fg-subtle"
                    }`}
                  >
                    {index < 9 ? <kbd className={STYLES.kbdBadge(isSelected)}>{index + 1}</kbd> : null}
                    <span className={STYLES.typeBadge(isSelected)}>{getClipTypeLabel(item)}</span>
                    <span className={`min-w-0 flex-1 truncate ${isSelected ? "font-medium text-pg-fg-muted" : "font-medium"}`}>
                      {item.sourceApp ?? "未知来源"}
                    </span>
                    <span className="flex shrink-0 items-center gap-1 font-medium">
                      <span className="tabular-nums">
                        {formatDateTime(item.lastUsedAt ?? item.createdAt)}
                      </span>
                      {item.isFavorited ? (
                        <span className={`${isSelected ? "text-[11px]" : "text-[12px]"} text-pg-favorite`}>★</span>
                      ) : null}
                    </span>
                  </div>
                </button>
              );
            })}
          </div>
          )}
        </div>
      </div>
    </div>
  );
}

