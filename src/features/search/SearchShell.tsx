import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { queryClient } from "../../app/queryClient";
import {
  hidePicker,
  hideSearch,
  hideTooltip,
  openEditorFromSearch,
  pasteItem,
  prepareSearchWindowDrag,
  setItemFavorited,
  showTooltip,
} from "../../bridge/commands";
import {
  CLIPS_CHANGED_EVENT,
  PICKER_CONFIRM_AS_FILE_EVENT,
  PICKER_CONFIRM_EVENT,
  PICKER_FAVORITE_EVENT,
  PICKER_NAVIGATE_EVENT,
  PICKER_OPEN_EDITOR_EVENT,
  PICKER_SELECT_INDEX_EVENT,
  SEARCH_EDIT_ITEM_EVENT,
  SEARCH_INPUT_RESUME_EVENT,
  SEARCH_INPUT_SUSPEND_EVENT,
  SEARCH_NAVIGATE_EVENT,
  SEARCH_PASTE_EVENT,
  SEARCH_SESSION_END_EVENT,
  SEARCH_SESSION_START_EVENT,
  SETTINGS_CHANGED_EVENT,
} from "../../bridge/events";
import { getImageUrl } from "../../bridge/imageUrl";
import { isTauriRuntime } from "../../bridge/runtime";
import {
  setCurrentWindowLogicalSizeBounds,
  setCurrentWindowLogicalSize,
  startCurrentWindowDragging,
} from "../../bridge/window";
import { getSettings } from "../../bridge/commands";
import { useItemDetailQuery } from "../../shared/queries/clipQueries";
import type {
  ClipItemDetail,
  ClipItemSummary,
  SearchQuickFilter,
  SearchResult,
} from "../../shared/types/clips";
import { getClipTypeLabel } from "../../shared/utils/clipDisplay";
import { formatDateTime } from "../../shared/utils/time";
import { LoadingSpinner } from "../../shared/ui/LoadingSpinner";
import { TOOLTIP_SHOW_DELAY_MS } from "../../shared/ui/tooltipConfig";
import { buildThemeCssVariables, DEFAULT_CUSTOM_THEME_COLORS } from "../../shared/themeColors";
import { getSearchKeyboardAction } from "./keyboard";
import {
  getSearchFilterCommitFocusTarget,
  getSearchFilterOptionAction,
  getSearchFilterTriggerAction,
} from "./filterKeyboard";
import { shouldPreventSearchItemMouseFocus } from "./itemPointer";
import {
  getSearchItemFavoritedState,
  setFavoritedOnDetail,
  setFavoritedOnSearchResult,
} from "./favoritedState";
import { buildTooltipHtml } from "../picker/tooltipHtml";
import { resolveTooltipShowPosition } from "../picker/tooltipState";
import { useQuery } from "@tanstack/react-query";
import {
  createSearchRecentQueryKey,
  createSearchSearchQueryKey,
  useSearchRecentQuery,
  useSearchSearchQuery,
} from "./queries";
import { getNextSearchNavigationIndex } from "./state";
import { useSearchStore } from "./store";
import type { SearchSession } from "./store";

const STYLES = {
  shell:
    "relative flex h-screen w-screen flex-col overflow-hidden bg-pg-canvas-default text-pg-fg-default",
  panel:
    "flex h-full w-full flex-col overflow-hidden border border-pg-border-default bg-pg-canvas-default shadow-[0_20px_60px_rgba(var(--pg-shadow-color),0.18)]",
  searchHeader:
    "flex items-center gap-3 border-b border-pg-border-subtle px-3 py-3",
  searchControl:
    "relative flex flex-1 items-center rounded-md border border-pg-border-subtle bg-pg-canvas-default px-2",
  searchControlIcon:
    "flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-pg-fg-muted",
  searchInput:
    "w-full appearance-none border-0 bg-transparent p-0 text-[17px] leading-6 outline-none shadow-none ring-0 placeholder:text-pg-fg-subtle focus:border-0 focus:outline-none focus:ring-0 focus-visible:border-0 focus-visible:outline-none focus-visible:ring-0",
  searchFilterDivider: "mx-2 h-5 w-px shrink-0 bg-pg-border-subtle",
  searchFilterButton:
    "flex h-9 shrink-0 items-center gap-2 rounded-md px-2.5 text-[13px] font-medium leading-5 text-pg-fg-default transition-colors hover:bg-pg-canvas-subtle focus:bg-pg-canvas-subtle focus:outline-none",
  searchFilterChevron:
    "h-3.5 w-3.5 text-pg-fg-subtle transition-transform duration-150",
  searchFilterPanel:
    "absolute right-0 top-[calc(100%+0.5rem)] z-30 min-w-[156px] overflow-hidden rounded-md border border-pg-border-default bg-pg-canvas-default shadow-[0_16px_40px_rgba(var(--pg-shadow-color),0.18)]",
  searchFilterOption: (active: boolean, selected: boolean) =>
    `flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left text-[13px] font-medium leading-5 transition-colors ${
      active ? "bg-pg-canvas-subtle text-pg-fg-default" : "text-pg-fg-muted"
    } ${selected ? "text-pg-accent-fg" : ""}`,
  listItemShell: (selected: boolean) =>
    `rounded-[9px] border transition-[border-color,background-color,box-shadow] ${
      selected
        ? "border-pg-accent-fg bg-pg-accent-subtle shadow-[inset_0_0_0_1px_rgba(var(--pg-shadow-color),0.04)]"
        : "border-pg-border-subtle bg-pg-canvas-subtle"
    }`,
  listItemLayout: (selected: boolean) =>
    `group grid w-full items-start gap-2.5 px-2 py-3 text-left transition-colors ${
      selected
        ? "grid-cols-[auto,minmax(0,1fr),auto]"
        : "grid-cols-[auto,minmax(0,1fr)]"
    } ${
      selected ? "" : "hover:bg-pg-canvas-inset"
    }`,
  glyphBox: (selected: boolean) =>
    `flex h-9 w-9 shrink-0 items-center justify-center rounded-[10px] text-sm font-semibold transition-colors ${
      selected
        ? "bg-pg-neutral-5 text-pg-fg-default shadow-[inset_0_0_0_1px_rgba(var(--pg-shadow-color),0.06)] dark:bg-pg-neutral-6"
        : "bg-pg-canvas-subtle text-pg-fg-muted group-hover:text-pg-fg-default"
    }`,
  selectedActions:
    "col-start-3 row-start-1 flex justify-end self-start pt-1",
  selectedActionStack:
    "flex items-center gap-1.5",
  inlineMetaRow:
    "mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-pg-fg-subtle",
  actionButton:
    "flex h-8 w-8 items-center justify-center rounded-md bg-pg-accent-emphasis text-pg-fg-on-emphasis transition-colors hover:opacity-90",
  actionButtonSecondary:
    "flex h-8 w-8 items-center justify-center rounded-md border border-pg-border-default text-pg-fg-default transition-colors hover:bg-pg-canvas-subtle",
};

const SEARCH_WINDOW_FIXED_WIDTH = 780;
const SEARCH_WINDOW_MAX_HEIGHT = 620;
const SEARCH_FILTER_PANEL_WINDOW_MARGIN = 8;
const SEARCH_IMAGE_THUMBNAIL_STYLE = {
  width: 36,
  height: 36,
} as const;

const FILTER_OPTIONS: Array<{ value: SearchQuickFilter; label: string }> = [
  { value: "all", label: "全部" },
  { value: "favorite", label: "收藏" },
  { value: "text", label: "文本" },
  { value: "image", label: "图片" },
  { value: "file", label: "文件" },
];

async function refreshSearchQueries() {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["detail"] }),
    queryClient.invalidateQueries({ queryKey: ["search-recent"] }),
    queryClient.invalidateQueries({ queryKey: ["search-query"] }),
  ]);
}

function getClipTypeGlyph(item: Pick<ClipItemSummary, "type">): string {
  if (item.type === "text") {
    return "Aa";
  }

  if (item.type === "image") {
    return "图";
  }

  if (item.type === "file") {
    return "档";
  }

  return "?";
}

function getSectionLabel(hasKeyword: boolean) {
  return hasKeyword ? "搜索结果" : "最近条目";
}

function getFilterLabel(filter: SearchQuickFilter) {
  return FILTER_OPTIONS.find((option) => option.value === filter)?.label ?? "全部";
}

function getAdjacentFilter(
  current: SearchQuickFilter,
  direction: 1 | -1,
): SearchQuickFilter {
  const currentIndex = FILTER_OPTIONS.findIndex((option) => option.value === current);
  const nextIndex =
    (currentIndex + direction + FILTER_OPTIONS.length) % FILTER_OPTIONS.length;
  return FILTER_OPTIONS[nextIndex]?.value ?? "all";
}

function getEmptyState(
  hasKeyword: boolean,
  activeFilter: SearchQuickFilter,
): { title: string; description: string } {
  if (hasKeyword) {
    return {
      title: "未找到匹配记录",
      description: "尝试调整搜索关键词",
    };
  }

  if (activeFilter !== "all") {
    return {
      title: "当前筛选下暂无记录",
      description: "尝试切换其他筛选或复制更多内容",
    };
  }

  return {
    title: "暂无剪贴板记录",
    description: "复制内容后使用 Alt+S 打开此窗口",
  };
}

function formatFileSize(bytes: number | null): string | null {
  if (!bytes || bytes <= 0) {
    return null;
  }

  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  const digits = unitIndex === 0 ? 0 : value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${units[unitIndex]}`;
}

function getItemDetailMeta(detail: ClipItemDetail | ClipItemSummary): string[] {
  const meta = [
    getClipTypeLabel(detail),
    detail.sourceApp ?? "未知来源",
    formatDateTime(detail.lastUsedAt ?? detail.createdAt),
  ];

  if (detail.type === "image" && detail.imageWidth && detail.imageHeight) {
    meta.push(`${detail.imageWidth} × ${detail.imageHeight}`);
  }

  const fileSizeLabel = formatFileSize(detail.fileSize);
  if (fileSizeLabel) {
    meta.push(fileSizeLabel);
  }

  if (detail.type === "file") {
    if (detail.fileCount > 0) {
      meta.push(`${detail.fileCount} 个文件`);
    }
    if (detail.directoryCount > 0) {
      meta.push(`${detail.directoryCount} 个文件夹`);
    }
  }

  return meta;
}

async function handleSearchWindowDragStart(
  event: MouseEvent<HTMLElement>,
) {
  const target = event.target as HTMLElement | null;
  if (
    !target ||
    target.closest("input, button, textarea, select, [data-no-window-drag='true']")
  ) {
    return;
  }

  try {
    await prepareSearchWindowDrag();
    await startCurrentWindowDragging();
  } catch (error) {
    console.warn("启动搜索窗口拖拽失败", error);
  }
}

export function SearchShell() {
  const {
    keyword,
    reset,
    selectedItemId,
    session,
    setKeyword,
    setSelectedItemId,
    setSession,
  } = useSearchStore();
  const tauriRuntime = isTauriRuntime();
  const shellRef = useRef<HTMLDivElement>(null);
  const headerRef = useRef<HTMLElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const sectionBarRef = useRef<HTMLDivElement>(null);
  const listContentRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const selectedItemIdRef = useRef<string | null>(selectedItemId);
  const itemRefs = useRef<(HTMLDivElement | null)[]>([]);
  const imageUrlCacheRef = useRef(new Map<string, string | null>());
  const imageUrlPendingRef = useRef(new Set<string>());
  const [inputSuspended, setInputSuspended] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [, setImageUrlVersion] = useState(0);
  const [activeFilter, setActiveFilter] = useState<SearchQuickFilter>("all");
  const [isFilterOpen, setIsFilterOpen] = useState(false);
  const [highlightedFilter, setHighlightedFilter] =
    useState<SearchQuickFilter>("all");
  const filterRootRef = useRef<HTMLDivElement>(null);
  const filterPanelRef = useRef<HTMLDivElement>(null);
  const filterTriggerRef = useRef<HTMLButtonElement>(null);
  const filterOptionRefs = useRef<
    Partial<Record<SearchQuickFilter, HTMLDivElement | null>>
  >({});
  const errorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const filterCloseTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const tooltipTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const tooltipRequestIdRef = useRef(0);
  const resizeFrameRef = useRef<number | null>(null);
  const lastAppliedWindowHeightRef = useRef<number | null>(null);
  const favoriteTogglePendingRef = useRef(false);
  const hasKeyword = keyword.trim().length > 0;
  const recentQuery = useSearchRecentQuery(activeFilter, !hasKeyword);
  const searchQuery = useSearchSearchQuery(keyword, activeFilter, hasKeyword);
  const settingsQuery = useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
    staleTime: 0,
    refetchOnWindowFocus: false,
  });
  const items = useMemo<ClipItemSummary[]>(
    () => (hasKeyword ? (searchQuery.data?.items ?? []) : (recentQuery.data?.items ?? [])),
    [hasKeyword, recentQuery.data?.items, searchQuery.data?.items],
  );
  const itemsRef = useRef<ClipItemSummary[]>(items);
  const restoreClipboardRef = useRef(settingsQuery.data?.restoreClipboardAfterPaste ?? true);
  const detailQuery = useItemDetailQuery(selectedItemId);

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

  // 清理错误定时器
  useEffect(() => {
    return () => {
      if (errorTimerRef.current) {
        clearTimeout(errorTimerRef.current);
      }
      if (filterCloseTimerRef.current) {
        clearTimeout(filterCloseTimerRef.current);
      }
      if (tooltipTimerRef.current) {
        clearTimeout(tooltipTimerRef.current);
      }
      if (resizeFrameRef.current) {
        cancelAnimationFrame(resizeFrameRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!isFilterOpen) {
      return;
    }

    const handlePointerDown = (event: globalThis.MouseEvent) => {
      const target = event.target as Node | null;
      if (filterRootRef.current && target && !filterRootRef.current.contains(target)) {
        setIsFilterOpen(false);
      }
    };

    document.addEventListener("mousedown", handlePointerDown);
    return () => document.removeEventListener("mousedown", handlePointerDown);
  }, [isFilterOpen]);

  useEffect(() => {
    if (!isFilterOpen) {
      return;
    }

    const frame = requestAnimationFrame(() => {
      filterOptionRefs.current[highlightedFilter]?.focus();
    });

    return () => cancelAnimationFrame(frame);
  }, [highlightedFilter, isFilterOpen]);

  // 显示临时错误消息（3秒后自动消失）
  const showError = (message: string) => {
    if (errorTimerRef.current) {
      clearTimeout(errorTimerRef.current);
    }
    setErrorMessage(message);
    errorTimerRef.current = setTimeout(() => {
      setErrorMessage(null);
    }, 3000);
  };

  const clearFilterCloseTimer = () => {
    if (filterCloseTimerRef.current) {
      clearTimeout(filterCloseTimerRef.current);
      filterCloseTimerRef.current = null;
    }
  };

  useEffect(() => {
    itemsRef.current = items;
    selectedItemIdRef.current = selectedItemId;
  }, [items, selectedItemId]);

  useEffect(() => {
    restoreClipboardRef.current = settingsQuery.data?.restoreClipboardAfterPaste ?? true;
  }, [settingsQuery.data?.restoreClipboardAfterPaste]);

  const handleItemMouseMove = (event: React.MouseEvent, item: ClipItemSummary) => {
    if (!tauriRuntime || item.type !== "image") {
      return;
    }

    const requestId = invalidateTooltipRequest();
    const clientPosition = { x: event.clientX, y: event.clientY };
    clearTooltipTimer();
    tooltipTimerRef.current = setTimeout(() => {
      tooltipTimerRef.current = null;
      void (async () => {
        const imageUrl = imageUrlCacheRef.current.get(item.id) ?? await getImageUrl(item.imagePath);
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
          buildThemeCssVariables(
            ((document.documentElement.dataset.theme as "dark" | "light") ?? "dark"),
            settingsQuery.data?.customThemeColors ?? DEFAULT_CUSTOM_THEME_COLORS,
          ),
        );
      })().catch((error) => {
        console.warn("[FloatPaste] Search tooltip 定位或显示失败:", error);
      });
    }, TOOLTIP_SHOW_DELAY_MS);
  };

  const handleItemMouseLeave = () => {
    if (!tauriRuntime) {
      return;
    }
    cancelTooltip();
  };

  useEffect(() => {
    items.forEach((item) => {
      if (
        item.type !== "image"
        || !item.imagePath
        || imageUrlCacheRef.current.has(item.id)
        || imageUrlPendingRef.current.has(item.id)
      ) {
        return;
      }

      imageUrlPendingRef.current.add(item.id);
      void getImageUrl(item.imagePath).then((imageUrl) => {
        imageUrlCacheRef.current.set(item.id, imageUrl);
        setImageUrlVersion((current) => current + 1);
      }).catch(() => {
        imageUrlCacheRef.current.set(item.id, null);
      }).finally(() => {
        imageUrlPendingRef.current.delete(item.id);
      });
    });
  }, [items]);

  // 当选中项改变时，自动滚动到视图
  useEffect(() => {
    if (!selectedItemId) {
      return;
    }
    const selectedIndex = items.findIndex((item) => item.id === selectedItemId);
    const currentItem = itemRefs.current[selectedIndex];
    if (currentItem) {
      currentItem.scrollIntoView({
        behavior: "auto",
        block: "nearest",
      });
    }
  }, [selectedItemId, items]);

  useEffect(() => {
    cancelTooltip();
  }, [activeFilter, items, selectedItemId]);

  useEffect(() => {
    return () => {
      cancelTooltip();
    };
  }, []);

  useEffect(() => {
    if (!tauriRuntime) {
      return;
    }

    const scheduleWindowSizeSync = () => {
      if (resizeFrameRef.current) {
        cancelAnimationFrame(resizeFrameRef.current);
      }

      resizeFrameRef.current = requestAnimationFrame(() => {
        resizeFrameRef.current = null;

        const shellRect = shellRef.current?.getBoundingClientRect();
        const shellHeight = Math.round(
          shellRect?.height ?? 0,
        );
        const headerHeight = headerRef.current?.offsetHeight ?? 0;
        const errorHeight = errorRef.current?.offsetHeight ?? 0;
        const sectionHeight = sectionBarRef.current?.offsetHeight ?? 0;
        const listContentHeight = Math.ceil(
          listContentRef.current?.getBoundingClientRect().height ?? 0,
        );
        const filterPanelBottom = isFilterOpen && shellRect && filterPanelRef.current
          ? Math.ceil(
            filterPanelRef.current.getBoundingClientRect().bottom
              - shellRect.top
              + SEARCH_FILTER_PANEL_WINDOW_MARGIN,
          )
          : 0;

        if (!headerHeight || !sectionHeight || (!listContentHeight && !filterPanelBottom)) {
          return;
        }

        const chromeHeight = 5 + headerHeight + errorHeight + sectionHeight;
        const contentHeight = chromeHeight + listContentHeight;
        const targetHeight = Math.min(
          SEARCH_WINDOW_MAX_HEIGHT,
          Math.max(contentHeight, filterPanelBottom),
        );

        if (lastAppliedWindowHeightRef.current === targetHeight && shellHeight === targetHeight) {
          return;
        }

        lastAppliedWindowHeightRef.current = targetHeight;
        const targetWidth = SEARCH_WINDOW_FIXED_WIDTH;
        void (async () => {
          await setCurrentWindowLogicalSizeBounds(
            SEARCH_WINDOW_FIXED_WIDTH,
            targetHeight,
            SEARCH_WINDOW_FIXED_WIDTH,
            targetHeight,
          );
          await setCurrentWindowLogicalSize(targetWidth, targetHeight);
        })().catch((error) => {
          console.warn("同步搜索窗口尺寸失败", error);
        });
      });
    };

    scheduleWindowSizeSync();

    if (typeof ResizeObserver === "undefined") {
      return;
    }

    const observer = new ResizeObserver(() => {
      scheduleWindowSizeSync();
    });
    const observedElements = [
      shellRef.current,
      headerRef.current,
      errorRef.current,
      sectionBarRef.current,
      listContentRef.current,
      filterPanelRef.current,
    ].filter((node): node is HTMLElement => node !== null);

    observedElements.forEach((node) => observer.observe(node));

    return () => {
      observer.disconnect();
      if (resizeFrameRef.current) {
        cancelAnimationFrame(resizeFrameRef.current);
        resizeFrameRef.current = null;
      }
    };
  }, [
    activeFilter,
    detailQuery.dataUpdatedAt,
    detailQuery.isLoading,
    errorMessage,
    isFilterOpen,
    items.length,
    selectedItemId,
    tauriRuntime,
  ]);

  useEffect(() => {
    if (!items.length) {
      setSelectedItemId(null);
      return;
    }

    if (selectedItemId && items.some((item) => item.id === selectedItemId)) {
      return;
    }

    if (session?.initialItemId && items.some((item) => item.id === session.initialItemId)) {
      setSelectedItemId(session.initialItemId);
      return;
    }

    setSelectedItemId(items[0]?.id ?? null);
  }, [items, selectedItemId, session?.initialItemId, setSelectedItemId]);

  useEffect(() => {
    if (searchInputRef.current) {
      searchInputRef.current.focus();
    }
  }, [session?.source]);

  const navigateSelection = (direction: "up" | "down") => {
    const currentItems = itemsRef.current;
    if (!currentItems.length) {
      return;
    }

    const nextIndex = getNextSearchNavigationIndex(
      currentItems,
      selectedItemIdRef.current,
      direction,
    );

    if (nextIndex >= 0) {
      const nextId = currentItems[nextIndex]?.id ?? null;
      selectedItemIdRef.current = nextId;
      setSelectedItemId(nextId);
    }
  };

  const closeFilterMenu = (focusTrigger: boolean) => {
    clearFilterCloseTimer();
    setIsFilterOpen(false);
    setHighlightedFilter(activeFilter);
    if (focusTrigger) {
      requestAnimationFrame(() => {
        filterTriggerRef.current?.focus();
      });
    }
  };

  const openFilterMenu = (initialFilter: SearchQuickFilter = activeFilter) => {
    clearFilterCloseTimer();
    setHighlightedFilter(initialFilter);
    setIsFilterOpen(true);
  };

  const commitFilter = (nextFilter: SearchQuickFilter) => {
    clearFilterCloseTimer();
    setActiveFilter(nextFilter);
    setHighlightedFilter(nextFilter);
    setIsFilterOpen(false);
    requestAnimationFrame(() => {
      if (getSearchFilterCommitFocusTarget() === "search-input") {
        searchInputRef.current?.focus();
      }
    });
  };

  async function forwardPickerNavigate(direction: "up" | "down") {
    try {
      await emitTo("picker", PICKER_NAVIGATE_EVENT, direction);
    } catch (error) {
      console.error("控制速贴面板失败", error);
    }
  }

  async function forwardPickerConfirm() {
    try {
      await emitTo("picker", PICKER_CONFIRM_EVENT);
    } catch (error) {
      console.error("控制速贴面板失败", error);
    }
  }

  async function forwardPickerOpenEditor() {
    try {
      await emitTo("picker", PICKER_OPEN_EDITOR_EVENT);
    } catch (error) {
      console.error("控制速贴面板失败", error);
    }
  }

  async function forwardPickerSelectIndex(index: number) {
    try {
      await emitTo("picker", PICKER_SELECT_INDEX_EVENT, index);
    } catch (error) {
      console.error("控制速贴面板失败", error);
    }
  }

  async function forwardPickerFavorite() {
    try {
      await emitTo("picker", PICKER_FAVORITE_EVENT);
    } catch (error) {
      console.error("控制速贴面板失败", error);
    }
  }

  async function closePickerFromSearch() {
    try {
      await hidePicker();
    } catch (error) {
      console.error("关闭速贴面板失败", error);
    }
  }

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let disposed = false;
    let offStart: (() => void) | undefined;
    let offClipsChanged: (() => void) | undefined;
    let offEnd: (() => void) | undefined;
    let offNavigate: (() => void) | undefined;
    let offEdit: (() => void) | undefined;
    let offPaste: (() => void) | undefined;
    let offSuspend: (() => void) | undefined;
    let offResume: (() => void) | undefined;
    let offSettingsChanged: (() => void) | undefined;

    const handleListenError = (eventName: string, error: unknown) => {
      console.error(`注册搜索窗口事件监听失败: ${eventName}`, error);
      if (!disposed) {
        showError("搜索窗口初始化失败，部分快捷操作可能不可用");
      }
    };

    void listen<{
      source: string;
      itemId?: string;
      initialKeyword?: string;
    }>(SEARCH_SESSION_START_EVENT, async (event) => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });

      setSession({
        source: "global" as const,
        initialItemId: event.payload.itemId,
        initialKeyword: event.payload.initialKeyword,
      } as SearchSession);
      setIsFilterOpen(false);
      setActiveFilter("all");
      setHighlightedFilter("all");
      setKeyword(event.payload.initialKeyword ?? "");
      setSelectedItemId(event.payload.itemId ?? null);
      setInputSuspended(false);
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      offStart = cleanup;
    }).catch((error) => {
      handleListenError(SEARCH_SESSION_START_EVENT, error);
    });

    void listen(CLIPS_CHANGED_EVENT, () => {
      void refreshSearchQueries().catch((error) => {
        console.error("刷新搜索结果失败", error);
        if (!disposed) {
          showError("刷新搜索结果失败，请稍后重试");
        }
      });
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      offClipsChanged = cleanup;
    }).catch((error) => {
      handleListenError(CLIPS_CHANGED_EVENT, error);
    });

    void listen(SEARCH_SESSION_END_EVENT, () => {
      setInputSuspended(false);
      setIsFilterOpen(false);
      setActiveFilter("all");
      setHighlightedFilter("all");
      reset();
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      offEnd = cleanup;
    }).catch((error) => {
      handleListenError(SEARCH_SESSION_END_EVENT, error);
    });

    void listen<string>(SEARCH_NAVIGATE_EVENT, (event) => {
      const currentItems = itemsRef.current;
      if (!currentItems.length) {
        return;
      }

      const nextIndex = getNextSearchNavigationIndex(
        currentItems,
        selectedItemIdRef.current,
        event.payload === "up" ? "up" : "down",
      );

      if (nextIndex >= 0) {
        setSelectedItemId(currentItems[nextIndex]?.id ?? null);
      }
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      offNavigate = cleanup;
    }).catch((error) => {
      handleListenError(SEARCH_NAVIGATE_EVENT, error);
    });

    void listen(SEARCH_EDIT_ITEM_EVENT, () => {
      void handleOpenEditor();
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      offEdit = cleanup;
    }).catch((error) => {
      handleListenError(SEARCH_EDIT_ITEM_EVENT, error);
    });

    void listen(SEARCH_PASTE_EVENT, () => {
      void handlePasteSelected();
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      offPaste = cleanup;
    }).catch((error) => {
      handleListenError(SEARCH_PASTE_EVENT, error);
    });

    void listen(SEARCH_INPUT_SUSPEND_EVENT, () => {
      setInputSuspended(true);
      searchInputRef.current?.blur();
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      offSuspend = cleanup;
    }).catch((error) => {
      handleListenError(SEARCH_INPUT_SUSPEND_EVENT, error);
    });

    void listen(SEARCH_INPUT_RESUME_EVENT, () => {
      setInputSuspended(false);
      searchInputRef.current?.focus();
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      offResume = cleanup;
    }).catch((error) => {
      handleListenError(SEARCH_INPUT_RESUME_EVENT, error);
    });

    void listen(SETTINGS_CHANGED_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      offSettingsChanged = cleanup;
    }).catch((error) => {
      handleListenError(SETTINGS_CHANGED_EVENT, error);
    });

    return () => {
      disposed = true;
      offStart?.();
      offClipsChanged?.();
      offEnd?.();
      offNavigate?.();
      offEdit?.();
      offPaste?.();
      offSuspend?.();
      offResume?.();
      offSettingsChanged?.();
    };
  }, [reset, setKeyword, setSelectedItemId, setSession]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      // 下拉菜单自己处理方向键、Tab 和 Enter，避免被全局搜索快捷键抢走。
      if (
        target?.closest("[data-search-filter-root='true']")
        && !event.ctrlKey
        && !event.metaKey
      ) {
        return;
      }

      if (inputSuspended && !event.isComposing) {
        if (event.key === "Escape") {
          event.preventDefault();
          void closePickerFromSearch();
          return;
        }

        if (event.key === "ArrowUp") {
          event.preventDefault();
          void forwardPickerNavigate("up");
          return;
        }

        if (event.key === "ArrowDown") {
          event.preventDefault();
          void forwardPickerNavigate("down");
          return;
        }

        if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
          event.preventDefault();
          void forwardPickerOpenEditor();
          return;
        }

        if ((event.ctrlKey || event.metaKey) && event.key === " ") {
          event.preventDefault();
          void forwardPickerFavorite();
          return;
        }

        if (event.key === "Enter") {
          event.preventDefault();
          if (event.shiftKey) {
            void emitTo("picker", PICKER_CONFIRM_AS_FILE_EVENT).catch((error) => {
              console.error("控制速贴面板失败", error);
            });
          } else {
            void forwardPickerConfirm();
          }
          return;
        }

        if (/^[1-9]$/.test(event.key)) {
          event.preventDefault();
          void forwardPickerSelectIndex(Number(event.key) - 1);
          return;
        }
      }

      const action = getSearchKeyboardAction({
        key: event.key,
        ctrlKey: event.ctrlKey,
        metaKey: event.metaKey,
        inputSuspended,
        isComposing: event.isComposing,
      });

      if (!action) {
        return;
      }

      event.preventDefault();

      switch (action) {
        case "navigate-up":
          navigateSelection("up");
          return;
        case "navigate-down":
          navigateSelection("down");
          return;
        case "paste":
          if (event.shiftKey) {
            const currentItem = itemsRef.current.find((item) => item.id === selectedItemIdRef.current);
            if (currentItem?.type === "image") {
              void handlePasteSelectedAsFile();
              return;
            }
          }
          void handlePasteSelected();
          return;
        case "edit-item":
          void handleOpenEditor();
          return;
        case "toggle-favorite":
          void handleToggleFavorited();
          return;
        case "close":
          void handleClose();
          return;
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [inputSuspended, keyword, selectedItemId]);

  async function handleClose() {
    cancelTooltip();
    try {
      await hideSearch();
    } catch (error) {
      console.error("关闭搜索窗口失败", error);
    }
  }

  async function handleOpenEditor() {
    const currentItem = itemsRef.current.find((item) => item.id === selectedItemIdRef.current);
    if (!currentItem) {
      return;
    }

    if (currentItem.type !== "text") {
      showError("当前只支持从文本条目进入独立编辑窗口");
      return;
    }

    cancelTooltip();
    try {
      await openEditorFromSearch(currentItem.id);
    } catch (error) {
      showError("打开编辑窗口失败，请稍后重试");
      console.error("打开编辑窗口失败", error);
    }
  }

  async function handlePasteSelected() {
    const currentItem = itemsRef.current.find((item) => item.id === selectedItemIdRef.current);
    if (!currentItem) {
      return;
    }

    try {
      await pasteItem(currentItem.id, {
        restoreClipboardAfterPaste: restoreClipboardRef.current,
        pasteToTarget: true,
      });
    } catch (error) {
      showError("执行粘贴失败，请稍后重试");
      console.error("执行粘贴失败", error);
    }
  }

  async function handlePasteSelectedAsFile() {
    const currentItem = itemsRef.current.find((item) => item.id === selectedItemIdRef.current);
    if (!currentItem || currentItem.type !== "image") {
      return;
    }

    try {
      await pasteItem(currentItem.id, {
        restoreClipboardAfterPaste: restoreClipboardRef.current,
        pasteToTarget: true,
        asFile: true,
      });
    } catch (error) {
      showError("执行粘贴失败，请稍后重试");
      console.error("执行粘贴失败", error);
    }
  }

  async function handleToggleFavorited() {
    const id = selectedItemIdRef.current;
    if (!id || favoriteTogglePendingRef.current) {
      return;
    }

    favoriteTogglePendingRef.current = true;

    try {
      const currentItem = itemsRef.current.find((item) => item.id === id);
      const currentDetail = detailQuery.data?.id === id ? detailQuery.data : undefined;
      const favored = getSearchItemFavoritedState(currentItem, currentDetail);
      const nextFavorited = !favored;
      await setItemFavorited(id, nextFavorited);
      queryClient.setQueryData<ClipItemDetail | undefined>(
        ["detail", id],
        (detail) => setFavoritedOnDetail(detail, id, nextFavorited),
      );
      queryClient.setQueriesData<SearchResult | undefined>(
        { queryKey: ["search-recent"] },
        (result) => setFavoritedOnSearchResult(result, id, nextFavorited),
      );
      queryClient.setQueriesData<SearchResult | undefined>(
        { queryKey: ["search-query"] },
        (result) => setFavoritedOnSearchResult(result, id, nextFavorited),
      );
      if (!nextFavorited && activeFilter === "favorite") {
        const activeQueryKey = hasKeyword
          ? createSearchSearchQueryKey(keyword, activeFilter)
          : createSearchRecentQueryKey(activeFilter);
        queryClient.setQueryData<SearchResult | undefined>(
          activeQueryKey,
          (result) => setFavoritedOnSearchResult(result, id, nextFavorited, {
            removeUnfavoritedItem: true,
          }),
        );
      }
      await refreshSearchQueries();
    } catch (error) {
      showError("更新收藏状态失败，请稍后重试");
      console.error("更新收藏状态失败", error);
    } finally {
      favoriteTogglePendingRef.current = false;
    }
  }

  const isLoading = hasKeyword ? searchQuery.isLoading : recentQuery.isLoading;
  const resultCountLabel = `${items.length} 条`;
  const emptyState = getEmptyState(hasKeyword, activeFilter);
  const activeFilterLabel = getFilterLabel(activeFilter);

  return (
    <div className={STYLES.shell} ref={shellRef}>
      <div className={STYLES.panel}>
        <header
          ref={headerRef}
          className={STYLES.searchHeader}
          onMouseDown={(event) => {
            void handleSearchWindowDragStart(event);
          }}
        >
          <div className={STYLES.searchControl} data-no-window-drag="true">
            <div className={STYLES.searchControlIcon}>
              <svg
                aria-hidden="true"
                className="h-5 w-5"
                fill="none"
                stroke="currentColor"
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth="1.8"
                viewBox="0 0 24 24"
              >
                <circle cx="11" cy="11" r="6.5" />
                <path d="m16 16 4.5 4.5" />
              </svg>
            </div>
            <input
              ref={searchInputRef}
              className={STYLES.searchInput}
              onChange={(event) => setKeyword(event.target.value)}
              placeholder="开始键入..."
              aria-label="搜索剪贴板记录"
              value={keyword}
            />
            <span aria-hidden="true" className={STYLES.searchFilterDivider} />
            <div
              ref={filterRootRef}
              className="relative ml-0.5 flex shrink-0 items-center"
              data-search-filter-root="true"
            >
              <button
                ref={filterTriggerRef}
                aria-controls="search-filter-listbox"
                aria-expanded={isFilterOpen}
                aria-haspopup="listbox"
                aria-label={`筛选剪贴板记录，当前为${activeFilterLabel}`}
                className={STYLES.searchFilterButton}
                onClick={() => {
                  if (isFilterOpen) {
                    closeFilterMenu(false);
                    return;
                  }
                  openFilterMenu(activeFilter);
                }}
                onKeyDown={(event) => {
                  const action = getSearchFilterTriggerAction({
                    key: event.key,
                    ctrlKey: event.ctrlKey,
                    metaKey: event.metaKey,
                  });

                  if (!action) {
                    return;
                  }

                  event.preventDefault();

                  if (action === "open-next") {
                    openFilterMenu(getAdjacentFilter(activeFilter, 1));
                    return;
                  }

                  if (action === "open-prev") {
                    openFilterMenu(getAdjacentFilter(activeFilter, -1));
                    return;
                  }

                  if (isFilterOpen) {
                    closeFilterMenu(false);
                    return;
                  }

                  openFilterMenu(activeFilter);
                }}
                type="button"
              >
                <span>{activeFilterLabel}</span>
                <svg
                  aria-hidden="true"
                  className={STYLES.searchFilterChevron}
                  style={{ transform: isFilterOpen ? "rotate(180deg)" : undefined }}
                  fill="none"
                  stroke="currentColor"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth="1.8"
                  viewBox="0 0 24 24"
                >
                  <path d="m7 10 5 5 5-5" />
                </svg>
              </button>
              {isFilterOpen ? (
                <div
                  className={STYLES.searchFilterPanel}
                  id="search-filter-listbox"
                  ref={filterPanelRef}
                  role="listbox"
                >
                  {FILTER_OPTIONS.map((option) => {
                    const isActive = highlightedFilter === option.value;
                    const isSelected = activeFilter === option.value;

                    return (
                      <div
                        key={option.value}
                        ref={(node) => {
                          filterOptionRefs.current[option.value] = node;
                        }}
                        aria-selected={isSelected}
                        className={STYLES.searchFilterOption(isActive, isSelected)}
                        onClick={() => commitFilter(option.value)}
                        onFocus={() => setHighlightedFilter(option.value)}
                        onKeyDown={(event) => {
                          const action = getSearchFilterOptionAction({
                            key: event.key,
                            ctrlKey: event.ctrlKey,
                            metaKey: event.metaKey,
                          });

                          if (action === null) {
                            if (event.key === "Tab") {
                              clearFilterCloseTimer();
                              filterCloseTimerRef.current = globalThis.setTimeout(() => {
                                setIsFilterOpen(false);
                              }, 0);
                            }
                            return;
                          }

                          event.preventDefault();

                          if (action === "next") {
                            setHighlightedFilter(getAdjacentFilter(option.value, 1));
                            return;
                          }

                          if (action === "prev") {
                            setHighlightedFilter(getAdjacentFilter(option.value, -1));
                            return;
                          }

                          if (action === "first") {
                            setHighlightedFilter(FILTER_OPTIONS[0]?.value ?? "all");
                            return;
                          }

                          if (action === "last") {
                            setHighlightedFilter(
                              FILTER_OPTIONS[FILTER_OPTIONS.length - 1]?.value ?? "all",
                            );
                            return;
                          }

                          if (action === "commit") {
                            commitFilter(option.value);
                            return;
                          }

                          if (action === "close") {
                            closeFilterMenu(true);
                            return;
                          }

                          if (event.key === "Tab") {
                            clearFilterCloseTimer();
                            filterCloseTimerRef.current = globalThis.setTimeout(() => {
                              setIsFilterOpen(false);
                            }, 0);
                          }
                        }}
                        onMouseEnter={() => setHighlightedFilter(option.value)}
                        role="option"
                        tabIndex={isActive ? 0 : -1}
                      >
                        {isSelected ? (
                          <svg
                            aria-hidden="true"
                            className="h-3.5 w-3.5 shrink-0"
                            fill="none"
                            stroke="currentColor"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                            strokeWidth="2"
                            viewBox="0 0 24 24"
                          >
                            <path d="M5 13l4 4L19 7" />
                          </svg>
                        ) : (
                          <span className="h-3.5 w-3.5 shrink-0" />
                        )}
                        <span>{option.label}</span>
                      </div>
                    );
                  })}
                </div>
              ) : null}
            </div>
          </div>
        </header>

        {errorMessage ? (
          <div
            ref={errorRef}
            className="border-b border-pg-danger-fg/20 bg-pg-danger-subtle px-5 py-2 text-sm text-pg-danger-fg"
          >
            {errorMessage}
          </div>
        ) : null}

        <div
          ref={sectionBarRef}
          className="flex items-center justify-between border-b border-pg-border-subtle px-5 py-3 text-xs font-medium text-pg-fg-muted"
        >
          <span>{getSectionLabel(hasKeyword)}</span>
          <span>{resultCountLabel}</span>
        </div>

        <main className="min-h-0 flex-1 overflow-y-auto [scrollbar-gutter:stable_both-edges]">
          <div ref={listContentRef} className="px-0.5 pb-1 pt-1.5">
            {isLoading ? (
              <div className="flex min-h-[160px] items-center justify-center py-12">
                <LoadingSpinner size="sm" text="加载中..." />
              </div>
            ) : items.length === 0 ? (
              <div className="flex min-h-[160px] flex-col items-center justify-center gap-1 px-4 py-12 text-center text-sm text-pg-fg-subtle">
                <span>{emptyState.title}</span>
                <span>{emptyState.description}</span>
              </div>
            ) : (
              <div className="space-y-1">
                {items.map((item, index) => {
                  const isSelected = selectedItemId === item.id;
                  const inlineDetail = isSelected ? detailQuery.data ?? item : null;
                  const isFavorited = getSearchItemFavoritedState(
                    item,
                    detailQuery.data?.id === item.id ? detailQuery.data : null,
                  );
                  const imageUrl = item.type === "image"
                    ? (imageUrlCacheRef.current.get(item.id) ?? null)
                    : null;
                  const itemMeta = getItemDetailMeta(inlineDetail ?? item);
                  const selectedPreviewText = detailQuery.isLoading
                    ? "正在载入条目详情..."
                    : detailQuery.data?.type === "text"
                      ? (detailQuery.data.fullText || detailQuery.data.contentPreview)
                      : item.contentPreview;
                  const previewText = isSelected ? selectedPreviewText : item.contentPreview;

                  return (
                    <div
                      ref={(el) => {
                        itemRefs.current[index] = el;
                      }}
                      className={STYLES.listItemShell(isSelected)}
                      key={item.id}
                      onMouseDown={(event) => {
                        if (shouldPreventSearchItemMouseFocus(event.button)) {
                          event.preventDefault();
                        }
                      }}
                      onMouseLeave={handleItemMouseLeave}
                      onMouseMove={(event) => {
                        if (item.type === "image") {
                          handleItemMouseMove(event, item);
                          return;
                        }
                        handleItemMouseLeave();
                      }}
                      onClick={() => {
                        selectedItemIdRef.current = item.id;
                        setSelectedItemId(item.id);
                      }}
                      onDoubleClick={() => {
                        setSelectedItemId(item.id);
                        selectedItemIdRef.current = item.id;
                        void handlePasteSelected();
                      }}
                      role="button"
                      tabIndex={-1}
                    >
                      <div className={STYLES.listItemLayout(isSelected)}>
                        <div>
                          {imageUrl ? (
                            <img
                              alt=""
                              className={`shrink-0 rounded-[8px] border object-cover ${
                                isSelected
                                  ? "border-pg-border-default bg-pg-canvas-default"
                                  : "border-pg-border-subtle bg-pg-canvas-subtle"
                              }`}
                              onError={() => {
                                imageUrlCacheRef.current.set(item.id, null);
                                setImageUrlVersion((current) => current + 1);
                              }}
                              src={imageUrl}
                              style={SEARCH_IMAGE_THUMBNAIL_STYLE}
                            />
                          ) : (
                            <div className={STYLES.glyphBox(isSelected)}>
                              {getClipTypeGlyph(item)}
                            </div>
                          )}
                        </div>
                        <div className="min-w-0 flex-1">
                          <div className="flex min-w-0 items-start gap-2">
                            <p
                              className={`min-w-0 flex-1 text-[15px] leading-6 ${
                                isSelected
                                  ? "line-clamp-3 whitespace-pre-wrap break-words font-medium text-pg-fg-default"
                                  : "truncate text-pg-fg-default"
                              }`}
                            >
                              {previewText}
                            </p>
                            <div className="flex shrink-0 items-center gap-2">
                              {isFavorited ? (
                                <span className="text-[12px] text-pg-favorite">★</span>
                              ) : null}
                            </div>
                          </div>
                          <div className={STYLES.inlineMetaRow}>
                            {itemMeta.map((meta, metaIndex) => (
                              <span key={`${item.id}-${meta}`}>
                                {metaIndex > 0 ? <span aria-hidden="true">• </span> : null}
                                {meta}
                              </span>
                            ))}
                            {isFavorited ? <span aria-hidden="true">• 已收藏</span> : null}
                          </div>
                        </div>
                        {isSelected ? (
                          <div className={STYLES.selectedActions}>
                            <div
                              className={STYLES.selectedActionStack}
                              onClick={(event) => event.stopPropagation()}
                              onDoubleClick={(event) => event.stopPropagation()}
                            >
                              <button
                                aria-label="粘贴当前条目"
                                className={STYLES.actionButton}
                                onMouseDown={(event) => {
                                  event.preventDefault();
                                  event.stopPropagation();
                                }}
                                onClick={() => void handlePasteSelected()}
                                title="粘贴"
                                type="button"
                              >
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
                                  <path d="M9.75 3h4.5A1.75 1.75 0 0 1 16 4.75V6H8V4.75A1.75 1.75 0 0 1 9.75 3Z" />
                                  <rect x="5" y="6" width="14" height="15" rx="2.75" />
                                  <path d="M9 11h6" />
                                  <path d="M9 15h6" />
                                </svg>
                              </button>
                              {(detailQuery.data?.type ?? item.type) === "image" ? (
                                <button
                                  aria-label="粘贴为文件路径"
                                  className={STYLES.actionButtonSecondary}
                                  onMouseDown={(event) => {
                                    event.preventDefault();
                                    event.stopPropagation();
                                  }}
                                  onClick={() => void handlePasteSelectedAsFile()}
                                  title="粘贴为路径"
                                  type="button"
                                >
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
                                    <path d="M4 7V4h16v3" />
                                    <path d="M9 20h6" />
                                    <path d="M12 4v16" />
                                  </svg>
                                </button>
                              ) : null}
                              {(detailQuery.data?.type ?? item.type) === "text" ? (
                                <button
                                  aria-label="编辑当前条目"
                                  className={STYLES.actionButtonSecondary}
                                  onMouseDown={(event) => {
                                    event.preventDefault();
                                    event.stopPropagation();
                                  }}
                                  onClick={() => void handleOpenEditor()}
                                  title="编辑"
                                  type="button"
                                >
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
                                    <path d="M4.75 19.25h3.5L18.5 9 15 5.5 4.75 15.75v3.5Z" />
                                    <path d="m13.75 6.75 3.5 3.5" />
                                    <path d="M4.75 19.25 8 19.2" />
                                  </svg>
                                </button>
                              ) : null}
                              <button
                                aria-label={isFavorited ? "取消收藏当前条目" : "收藏当前条目"}
                                className={STYLES.actionButtonSecondary}
                                onMouseDown={(event) => {
                                  event.preventDefault();
                                  event.stopPropagation();
                                }}
                                onClick={() => void handleToggleFavorited()}
                                title={isFavorited ? "取消收藏" : "收藏"}
                                type="button"
                              >
                                {isFavorited ? (
                                  <svg
                                    aria-hidden="true"
                                    className="h-4 w-4 text-pg-favorite"
                                    fill="currentColor"
                                    viewBox="0 0 24 24"
                                  >
                                    <path d="m12 3.85 2.55 5.17 5.71.83-4.13 4.03.98 5.69L12 16.89 6.89 19.57l.98-5.69-4.13-4.03 5.71-.83L12 3.85Z" />
                                  </svg>
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
                                    <path d="m12 3.85 2.55 5.17 5.71.83-4.13 4.03.98 5.69L12 16.89 6.89 19.57l.98-5.69-4.13-4.03 5.71-.83L12 3.85Z" />
                                  </svg>
                                )}
                              </button>
                            </div>
                          </div>
                        ) : null}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </main>
      </div>
    </div>
  );
}
