import { useEffect, useMemo, useRef, useState } from "react";
import { queryClient } from "../../app/queryClient";
import {
  hidePicker,
  openEditorFromPicker,
  pasteItem,
  setItemFavorited,
} from "../../bridge/commands";
import {
  CLIPS_CHANGED_EVENT,
  PICKER_CONFIRM_AS_FILE_EVENT,
  PICKER_CONFIRM_EVENT,
  PICKER_FAVORITE_EVENT,
  PICKER_NAVIGATE_EVENT,
  PICKER_OPEN_EDITOR_EVENT,
  PICKER_SELECT_INDEX_EVENT,
  PICKER_SESSION_END_EVENT,
  PICKER_SESSION_START_EVENT,
  SETTINGS_CHANGED_EVENT,
} from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import type { ClipItemSummary, ClipsChangedPayload } from "../../shared/types/clips";
import { useAppEvent } from "../../shared/hooks/useAppEvent";
import { useHoverTooltip } from "../../shared/hooks/useHoverTooltip";
import { useImageUrlCache } from "../../shared/hooks/useImageUrlCache";
import { getItemDetail } from "../../bridge/commands";
import { getClipTypeLabel } from "../../shared/utils/clipDisplay";
import { formatDateTime } from "../../shared/utils/time";
import { getErrorMessage } from "../../shared/utils/error";
import { LoadingSpinner } from "../../shared/ui/LoadingSpinner";
import { WindowResizeHandles, type WindowResizeHandle } from "../../shared/ui/WindowResizeHandles";
import {
  DEFAULT_PICKER_RECORD_LIMIT,
  normalizePickerRecordLimit,
  usePickerRecentQuery,
  usePickerSettingsQuery,
} from "./queries";
import { toggleFavoriteSelection } from "./favoriteToggle";
import { PICKER_IMAGE_THUMBNAIL_STYLE } from "./previewLayout";
import { buildTooltipHtml } from "../../shared/tooltip/tooltipHtml";
import { invalidateSettings } from "../../shared/queries/settingsQuery";
import { queryKeys } from "../../shared/queries/queryKeys";
import { applyClipsChanged } from "../../shared/queries/clipsCache";

const STYLES = {
  container:
    "flex h-screen w-screen flex-col overflow-hidden rounded-lg border border-pg-border-muted bg-pg-canvas-default",
  header:
    "flex shrink-0 items-center justify-between border-b border-pg-border-subtle bg-pg-canvas-default px-3 py-1.5",
  headerDot:
    "h-2 w-2 rounded-full bg-pg-accent-fg shadow-[0_0_0_3px_rgba(var(--pg-accent-rgb),0.10)]",
  headerMessage: (tone: PickerMessageTone) =>
    `ml-2 rounded-[4px] px-1.5 py-0.5 text-[10px] font-medium leading-none ${
      tone === "error"
        ? "bg-pg-danger-subtle text-pg-danger-fg"
        : "bg-pg-success-subtle text-pg-success-fg"
    }`,
  itemButton: (selected: boolean, favorited: boolean) =>
    `group relative flex w-full flex-col gap-1.5 rounded-lg px-1.5 py-2 text-left transition-colors border focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-pg-accent-fg focus-visible:ring-offset-2 ${
      selected
        ? "border-[color:rgba(var(--pg-accent-rgb),0.35)] bg-pg-accent-subtle shadow-[0_1px_0_rgba(var(--pg-shadow-color),0.14),inset_0_0_0_1px_rgba(var(--pg-accent-rgb),0.08)]"
        : favorited
          ? "border-pg-border-subtle border-l-[3px] border-l-pg-accent-fg bg-pg-canvas-subtle hover:border-pg-border-default hover:bg-pg-canvas-inset"
          : "bg-pg-canvas-subtle border-pg-border-subtle hover:border-pg-border-default hover:bg-pg-canvas-inset"
    }`,
  itemContent: (selected: boolean, favorited: boolean) =>
    `${selected ? "text-pg-fg-default font-semibold" : favorited ? "text-pg-fg-default font-medium" : "text-pg-fg-muted font-medium"} line-clamp-4 text-[13px] leading-[1.55] tracking-tight break-words [overflow-wrap:anywhere] whitespace-pre-wrap transition-colors`,
  kbdBadge: (selected: boolean) =>
    `inline-flex h-[18px] min-w-[18px] px-1.5 items-center justify-center rounded-[4px] font-mono text-[9px] font-bold transition-colors ${
      selected
        ? "bg-pg-accent-fg text-pg-fg-on-emphasis shadow-[inset_0_0_0_1px_rgba(255,255,255,0.10)]"
        : "bg-pg-canvas-subtle text-pg-fg-subtle group-hover:bg-pg-neutral-3 group-hover:text-pg-fg-muted"
    }`,
  typeBadge: (selected: boolean) =>
    `shrink-0 rounded-[4px] px-1.5 py-0.5 text-[10px] font-medium ${
      selected ? "bg-pg-canvas-default text-pg-fg-muted" : "bg-pg-neutral-3 text-pg-fg-subtle"
    }`,
};

/** picker 头部消息：成功/失败分别着色，替代原先统一的 accent 蓝底 */
export type PickerMessageTone = "success" | "error";

export type PickerMessage = {
  text: string;
  tone: PickerMessageTone;
};

const PICKER_RESIZE_HANDLES: WindowResizeHandle[] = [
  {
    key: "north-left",
    direction: "North",
    className: "absolute left-3 right-[calc(50%+2.25rem)] top-0 z-20 h-2 cursor-ns-resize",
  },
  {
    key: "north-right",
    direction: "North",
    className: "absolute left-[calc(50%+2.25rem)] right-3 top-0 z-20 h-2 cursor-ns-resize",
  },
  {
    key: "south",
    direction: "South",
    className: "absolute inset-x-3 bottom-0 z-20 h-2 cursor-ns-resize",
  },
  {
    key: "west",
    direction: "West",
    className:
      "absolute inset-y-3 left-0 z-20 w-1 cursor-ew-resize transition-colors hover:bg-pg-accent-subtle",
  },
  {
    key: "east",
    direction: "East",
    className:
      "absolute inset-y-3 right-0 z-20 w-1 cursor-ew-resize transition-colors hover:bg-pg-accent-subtle",
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
  // 列表查询不等设置就绪：默认 limit 先行渲染，设置到达后若 limit 不同会自动换 key 重查
  const recent = usePickerRecentQuery(pickerRecordLimit);
  const digitShortcutsEnabled = settings.data?.pickerDigitShortcutsEnabled ?? true;
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [lastMessage, setLastMessage] = useState<PickerMessage | null>(null);
  const itemsRef = useRef<ClipItemSummary[]>([]);
  const selectedIndexRef = useRef(0);
  // 选中项的 id 锚点：新剪贴内容插到列表头部时按 id 恢复，避免选区漂移到别的条目
  const selectedIdRef = useRef<string | null>(null);
  // 会话/设置刷新后的列表数据到达时重置选中到顶部，期间不按旧锚点恢复
  const expectResetRef = useRef(false);
  const restoreClipboardRef = useRef(settings.data?.restoreClipboardAfterPaste ?? true);
  const favoriteTogglePendingRef = useRef(false);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const listScrollRef = useRef<HTMLDivElement | null>(null);

  const items = useMemo(() => recent.data ?? [], [recent.data]);
  const imageCache = useImageUrlCache(items);

  const { cancelTooltip, handleMouseMove: handleItemMouseMove } = useHoverTooltip({
    enabled: tauriRuntime,
    customThemeColors: settings.data?.customThemeColors,
    buildHtml: async (item, requestId) => {
      // 列表载荷不含全文截断，tooltip 显示前按需请求 detail（React Query 缓存复用）
      const [imageUrl, detail] = await Promise.all([
        imageCache.resolve(item),
        queryClient
          .fetchQuery({
            queryKey: queryKeys.clipDetail(item.id),
            queryFn: () => getItemDetail(item.id),
            staleTime: 30_000,
          })
          .catch(() => null),
      ]);
      return buildTooltipHtml(item, {
        imageUrl,
        requestId,
        fullText: detail?.fullText ?? null,
      });
    },
  });

  const confirmSelection = async (index: number, asFile = false) => {
    const item = itemsRef.current[index];
    if (!item) {
      return;
    }

    cancelTooltip();

    try {
      const result = await pasteItem(item.id, {
        restoreClipboardAfterPaste: restoreClipboardRef.current,
        pasteToTarget: true,
        ...(asFile && item.type === "image" ? { asFile: true } : {}),
      });
      if (!result.success) {
        setLastMessage({ text: result.message || "粘贴失败，请稍后重试", tone: "error" });
        return;
      }
    } catch (error) {
      setLastMessage({
        text: `粘贴失败：${getErrorMessage(error, "请稍后重试")}`,
        tone: "error",
      });
      return;
    }
    // picker 紧接着会被隐藏，不清空的话下次打开时会闪过旧消息
    setLastMessage(null);
  };

  const handleOpenEditor = async () => {
    const item = itemsRef.current[selectedIndexRef.current];
    if (!item) {
      return;
    }

    cancelTooltip();

    try {
      await openEditorFromPicker(item.id);
    } catch (error) {
      setLastMessage({
        text: `打开编辑器失败：${getErrorMessage(error, "请稍后重试")}`,
        tone: "error",
      });
    }
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
        await queryClient.invalidateQueries({ queryKey: queryKeys.pickerRecents });
      },
      setLastMessage,
      onError: (error) => {
        console.error("更新收藏状态失败", error);
        setLastMessage({ text: "更新收藏失败，请稍后重试", tone: "error" });
      },
    });
  };

  useEffect(() => {
    itemsRef.current = items;
  }, [items]);

  useEffect(() => {
    restoreClipboardRef.current = settings.data?.restoreClipboardAfterPaste ?? true;
  }, [settings.data?.restoreClipboardAfterPaste]);

  useEffect(() => {
    selectedIndexRef.current = selectedIndex;
  }, [selectedIndex]);

  useEffect(() => {
    if (items.length === 0) {
      return;
    }
    if (expectResetRef.current) {
      expectResetRef.current = false;
      selectedIdRef.current = items[0]?.id ?? null;
      if (selectedIndexRef.current !== 0) {
        selectedIndexRef.current = 0;
        setSelectedIndex(0);
      }
      return;
    }
    const desiredId = selectedIdRef.current;
    if (desiredId) {
      const restoredIndex = items.findIndex((item) => item.id === desiredId);
      if (restoredIndex >= 0) {
        if (restoredIndex !== selectedIndexRef.current) {
          selectedIndexRef.current = restoredIndex;
          setSelectedIndex(restoredIndex);
        }
        return;
      }
      // 锚点条目已被删除：落到当前 index 指向的条目上
    }
    if (selectedIndexRef.current >= items.length) {
      selectedIndexRef.current = 0;
      setSelectedIndex(0);
    }
    selectedIdRef.current = items[selectedIndexRef.current]?.id ?? null;
  }, [items]);

  // 用户导航（方向键/数字键/点击）后更新 id 锚点；
  // 列表变化时的恢复逻辑由上面的 effect 负责，两者不要合并
  useEffect(() => {
    selectedIdRef.current = itemsRef.current[selectedIndex]?.id ?? null;
  }, [selectedIndex]);

  useEffect(() => {
    const currentItem = itemRefs.current[selectedIndex];
    if (currentItem) {
      currentItem.scrollIntoView({
        behavior: "auto",
        block: "nearest",
      });
    }
  }, [selectedIndex]);

  useAppEvent(PICKER_SESSION_END_EVENT, () => {
    cancelTooltip();
    selectedIndexRef.current = 0;
    selectedIdRef.current = null;
    setSelectedIndex(0);
  });

  useAppEvent(PICKER_SESSION_START_EVENT, async () => {
    // 设置缓存由 settings://changed 事件失效（staleTime 5 分钟），打开面板只刷新列表
    expectResetRef.current = true;
    await queryClient.invalidateQueries({ queryKey: queryKeys.pickerRecents });

    cancelTooltip();
    selectedIndexRef.current = 0;
    setSelectedIndex(0);
    setLastMessage(null);
    // 窗口通过 hide/show 复用，DOM 滚动位置会被保留。
    // 每次打开都把列表滚回顶部，避免停留在上次关闭时的位置。
    listScrollRef.current?.scrollTo({ top: 0 });
  });

  useAppEvent<ClipsChangedPayload>(CLIPS_CHANGED_EVENT, (payload) => {
    applyClipsChanged(payload);
  });

  useAppEvent(SETTINGS_CHANGED_EVENT, async () => {
    expectResetRef.current = true;
    await invalidateSettings(queryClient);
    await queryClient.invalidateQueries({ queryKey: queryKeys.pickerRecents });

    selectedIndexRef.current = 0;
    setSelectedIndex(0);
  });

  useAppEvent<string>(PICKER_NAVIGATE_EVENT, (direction) => {
    const itemCount = itemsRef.current.length;
    if (!itemCount) {
      return;
    }

    setSelectedIndex((current) => {
      const nextIndex =
        direction === "up" ? (current - 1 + itemCount) % itemCount : (current + 1) % itemCount;
      selectedIndexRef.current = nextIndex;
      return nextIndex;
    });
  });

  useAppEvent(PICKER_CONFIRM_EVENT, () => {
    void confirmSelection(selectedIndexRef.current);
  });

  useAppEvent(PICKER_CONFIRM_AS_FILE_EVENT, () => {
    void confirmSelection(selectedIndexRef.current, true);
  });

  useAppEvent<number>(PICKER_SELECT_INDEX_EVENT, (index) => {
    const itemCount = itemsRef.current.length;
    if (!itemCount) {
      return;
    }

    const clampedIndex = Math.max(0, Math.min(index, itemCount - 1));
    selectedIndexRef.current = clampedIndex;
    setSelectedIndex(clampedIndex);
    void confirmSelection(clampedIndex);
  });

  useAppEvent(PICKER_OPEN_EDITOR_EVENT, () => {
    void handleOpenEditor();
  });

  useAppEvent(PICKER_FAVORITE_EVENT, () => {
    void handleToggleFavorite();
  });

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

      if ((event.ctrlKey || event.metaKey) && event.key === " ") {
        event.preventDefault();
        void handleToggleFavorite();
        return;
      }

      if (event.key === "Enter") {
        event.preventDefault();
        const item = itemsRef.current[selectedIndexRef.current];
        const asFile = event.shiftKey && item?.type === "image";
        void confirmSelection(selectedIndexRef.current, asFile);
        return;
      }

      if (digitShortcutsEnabled && /^[1-9]$/.test(event.key)) {
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
  }, [tauriRuntime, digitShortcutsEnabled]);

  const handleItemMouseLeave = () => {
    if (!tauriRuntime) return;
    cancelTooltip();
  };

  useEffect(() => {
    return () => {
      cancelTooltip();
    };
  }, []);

  return (
    <div className="m-0 h-screen w-screen select-none overflow-hidden bg-transparent p-0 text-pg-fg-default">
      <div className={STYLES.container}>
        {tauriRuntime ? (
          <WindowResizeHandles handles={PICKER_RESIZE_HANDLES} errorLabel="速贴" />
        ) : null}

        <div className={STYLES.header}>
          <div className="flex min-w-0 flex-1 items-center gap-2" data-tauri-drag-region>
            <div aria-hidden="true" className={STYLES.headerDot} />
            <span className="text-[11px] font-semibold tracking-[0.02em] text-pg-fg-muted">
              FloatPaste
            </span>
            {lastMessage ? (
              <span
                className={STYLES.headerMessage(lastMessage.tone)}
                role={lastMessage.tone === "error" ? "alert" : "status"}
              >
                {lastMessage.text}
              </span>
            ) : null}
          </div>
        </div>

        <div className="flex min-h-0 flex-1 flex-col rounded-b-lg bg-pg-canvas-default px-0 py-1.5">
          {recent.isLoading ? (
            <div className="flex h-full items-center justify-center">
              <LoadingSpinner size="sm" text="正在加载记录..." />
            </div>
          ) : recent.isError && items.length === 0 ? (
            <div className="flex flex-1 flex-col items-center justify-center gap-1.5 py-8">
              <p className="text-sm text-pg-fg-muted">记录加载失败</p>
              <p className="text-xs leading-relaxed text-pg-fg-subtle">
                {getErrorMessage(recent.error, "请稍后重试")}
              </p>
              <button
                className="mt-1 rounded-lg border border-pg-border-default px-3 py-1.5 text-xs font-medium text-pg-fg-muted transition-colors hover:bg-pg-canvas-subtle hover:text-pg-fg-default"
                onClick={() => void recent.refetch()}
                type="button"
              >
                重试
              </button>
            </div>
          ) : items.length === 0 ? (
            <div className="flex flex-col items-center justify-center flex-1 gap-1 py-8">
              <p className="text-sm text-pg-fg-muted">暂无剪贴板记录</p>
              <p className="text-xs text-pg-fg-subtle">
                复制内容后按 {settings.data?.shortcut ?? "Alt+Q"} 打开此面板
              </p>
            </div>
          ) : (
            <div
              ref={listScrollRef}
              className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden [scrollbar-gutter:stable]"
            >
              <div className="grid gap-1 pl-[14px] pr-1 transition-colors">
                {items.map((item, index) => {
                  const isSelected = index === selectedIndex;
                  const imageUrl = item.type === "image" ? imageCache.getCached(item.id) : null;
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
                            className={`mt-0.5 shrink-0 rounded-md border object-contain ${
                              isSelected
                                ? "border-pg-border-default bg-pg-canvas-default"
                                : "border-pg-border-subtle bg-pg-canvas-subtle"
                            }`}
                            decoding="async"
                            loading="lazy"
                            onError={() => imageCache.markError(item.id)}
                            src={imageUrl}
                            style={PICKER_IMAGE_THUMBNAIL_STYLE}
                          />
                        ) : null}
                        <div className="min-w-0 flex-1">
                          <span className={STYLES.itemContent(isSelected, item.isFavorited)}>
                            {item.contentPreview}
                          </span>
                        </div>
                      </div>

                      <div
                        className={`flex w-full flex-wrap items-center gap-x-2 gap-y-1 text-[10px] leading-none transition-colors ${
                          isSelected ? "text-pg-fg-muted" : "text-pg-fg-subtle"
                        }`}
                      >
                        <div className="flex min-w-0 flex-1 items-center gap-2">
                          {index < 9 && digitShortcutsEnabled ? (
                            <kbd className={STYLES.kbdBadge(isSelected)}>{index + 1}</kbd>
                          ) : null}
                          <span className={STYLES.typeBadge(isSelected)}>
                            {getClipTypeLabel(item)}
                          </span>
                          <span
                            className={`min-w-0 flex-1 truncate ${isSelected ? "font-medium text-pg-fg-muted" : "font-medium"}`}
                          >
                            {item.sourceApp ?? "未知来源"}
                          </span>
                        </div>
                        <span className="ml-auto flex shrink-0 items-center gap-1 whitespace-nowrap font-medium">
                          <span className="tabular-nums">
                            {formatDateTime(item.lastUsedAt ?? item.createdAt)}
                          </span>
                          {item.isFavorited ? (
                            <span
                              className={`${isSelected ? "text-[11px]" : "text-[12px]"} text-pg-favorite`}
                            >
                              ★
                            </span>
                          ) : null}
                        </span>
                      </div>

                      {isSelected && item.tags.length > 0 ? (
                        <div className="mt-1 flex flex-wrap items-center gap-1">
                          {item.tags.slice(0, 2).map((tagName) => (
                            <span
                              className="rounded-full bg-pg-canvas-inset px-1.5 py-0.5 text-[10px] leading-4 text-pg-fg-muted"
                              key={tagName}
                            >
                              {tagName}
                            </span>
                          ))}
                          {item.tags.length > 2 ? (
                            <span className="text-[10px] leading-4 text-pg-fg-subtle">
                              +{item.tags.length - 2}
                            </span>
                          ) : null}
                        </div>
                      ) : null}
                    </button>
                  );
                })}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
