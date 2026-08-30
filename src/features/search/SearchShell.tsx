import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { emitTo } from "@tauri-apps/api/event";
import type { InfiniteData } from "@tanstack/react-query";
import { queryClient } from "../../app/queryClient";
import {
  deleteItem,
  hidePicker,
  hideSearch,
  openEditorFromSearch,
  pasteItem,
  prepareSearchWindowDrag,
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
  SEARCH_EDIT_ITEM_EVENT,
  SEARCH_INPUT_RESUME_EVENT,
  SEARCH_INPUT_SUSPEND_EVENT,
  SEARCH_NAVIGATE_EVENT,
  SEARCH_PASTE_EVENT,
  SEARCH_SESSION_END_EVENT,
  SEARCH_SESSION_START_EVENT,
  SETTINGS_CHANGED_EVENT,
  TAGS_CHANGED_EVENT,
} from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import { useAppEvent } from "../../shared/hooks/useAppEvent";
import { useArmedConfirm } from "../../shared/hooks/useArmedConfirm";
import { useHoverTooltip } from "../../shared/hooks/useHoverTooltip";
import { useImageUrlCache } from "../../shared/hooks/useImageUrlCache";
import {
  setCurrentWindowLogicalSizeBounds,
  setCurrentWindowLogicalSize,
  startCurrentWindowDragging,
} from "../../bridge/window";
import { invalidateSettings, useSettingsQuery } from "../../shared/queries/settingsQuery";
import { invalidateClipQueries, useItemDetailQuery } from "../../shared/queries/clipQueries";
import { queryKeys } from "../../shared/queries/queryKeys";
import { useTagsQuery } from "../../shared/queries/tagQueries";
import { applyClipsChanged } from "../../shared/queries/clipsCache";
import type {
  ClipItemDetail,
  ClipItemSummary,
  ClipsChangedPayload,
  SearchQuickFilter,
  SearchResult,
} from "../../shared/types/clips";
import { formatDateTime } from "../../shared/utils/time";
import { getErrorMessage } from "../../shared/utils/error";
import { ClipTypeIcon } from "../../shared/ui/icons";
import { LoadingSpinner } from "../../shared/ui/LoadingSpinner";
import { getSearchKeyboardAction } from "./keyboard";
import { shouldPreventSearchItemMouseFocus } from "./itemPointer";
import {
  getSearchItemFavoritedState,
  setFavoritedOnDetail,
  setFavoritedOnSearchResult,
} from "./favoritedState";
import { buildTooltipHtml } from "../../shared/tooltip/tooltipHtml";
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
    "flex h-full w-full flex-col overflow-hidden border border-pg-border-window bg-pg-canvas-default shadow-[0_20px_60px_rgba(var(--pg-shadow-color),0.18)]",
  searchHeader: "flex items-center gap-3 px-4 py-3",
  searchControl: (suspended: boolean) =>
    `relative flex flex-1 items-center rounded-md px-2 transition-colors ${
      // 无边框设计：键盘交出速贴面板时仅用底色差异提示状态
      suspended ? "bg-pg-canvas-subtle" : ""
    }`,
  searchControlIcon:
    "flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-pg-fg-muted",
  searchInput:
    "w-full appearance-none border-0 bg-transparent p-0 text-[17px] leading-6 outline-none shadow-none ring-0 placeholder:text-pg-fg-subtle focus:border-0 focus:outline-none focus:ring-0 focus-visible:border-0 focus-visible:outline-none focus-visible:ring-0",
  searchClearButton:
    "flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-pg-fg-subtle transition-colors hover:bg-pg-canvas-subtle hover:text-pg-fg-default",
  searchFilterDivider: "mx-1 h-5 w-px shrink-0 bg-pg-border-subtle",
  resultCount: "shrink-0 whitespace-nowrap text-xs tabular-nums text-pg-fg-subtle",
  // 全窗口仅存的分隔线：列表区的上边界
  filterRow:
    "flex shrink-0 items-center gap-1 overflow-x-auto border-b border-pg-border-subtle px-4 py-1.5 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
  filterChip: (selected: boolean) =>
    `shrink-0 rounded-md px-2.5 py-1 text-xs font-medium leading-4 transition-colors ${
      selected
        ? "bg-pg-accent-subtle text-pg-accent-fg"
        : "text-pg-fg-muted hover:bg-pg-canvas-subtle hover:text-pg-fg-default"
    }`,
  filterRowDivider: "mx-1 h-4 w-px shrink-0 bg-pg-border-subtle",
  tagChip: (selected: boolean) =>
    `shrink-0 rounded-full px-2.5 py-1 text-[12px] leading-4 transition-colors focus:outline-none ${
      selected
        ? "bg-pg-accent-subtle text-pg-accent-fg"
        : "text-pg-fg-muted hover:bg-pg-canvas-subtle hover:text-pg-fg-default"
    }`,
  listItemShell: (selected: boolean) =>
    `group relative rounded-lg border-l-2 transition-colors ${
      selected
        ? "border-l-pg-accent-fg bg-pg-accent-subtle"
        : "border-l-transparent hover:bg-pg-canvas-subtle"
    }`,
  listItemLayout: () =>
    "grid w-full grid-cols-[auto,minmax(0,1fr)] items-start gap-3 py-2.5 pl-3 pr-3 text-left",
  glyphBox: (selected: boolean) =>
    `flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-pg-canvas-inset transition-colors ${
      selected ? "text-pg-fg-default" : "text-pg-fg-muted group-hover:text-pg-fg-default"
    }`,
  glyphIcon: "h-[18px] w-[18px]",
  imageThumb: (selected: boolean) =>
    `shrink-0 rounded-md border object-cover ${
      selected ? "border-pg-border-default" : "border-pg-border-subtle"
    }`,
  imagePreviewLarge:
    "mt-1.5 max-h-[120px] max-w-full rounded-md border border-pg-border-subtle object-contain",
  selectedActions:
    "absolute right-2.5 top-1.5 z-10 flex items-center gap-1 rounded-md border border-pg-border-subtle bg-pg-canvas-default p-0.5 shadow-pg-md",
  hoverPasteButton:
    "absolute right-2.5 top-1.5 z-10 flex h-7 items-center gap-1 rounded-md border border-pg-border-default bg-pg-canvas-default px-2 text-xs font-medium text-pg-fg-default opacity-0 shadow-pg-sm transition-opacity focus-visible:opacity-100 group-hover:opacity-100",
  inlineMetaRow: "mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-pg-fg-subtle",
  itemTagChips: "mt-1 flex flex-wrap items-center gap-1",
  itemTagChip:
    "rounded-full bg-pg-canvas-inset px-1.5 py-0.5 text-[11px] leading-4 text-pg-fg-muted",
  itemTagOverflow: "text-[11px] leading-4 text-pg-fg-subtle",
  actionButton:
    "flex h-7 w-7 items-center justify-center rounded-md bg-pg-accent-emphasis text-pg-fg-on-emphasis transition-colors hover:opacity-90",
  actionButtonSecondary:
    "flex h-7 w-7 items-center justify-center rounded-md text-pg-fg-default transition-colors hover:bg-pg-canvas-subtle",
  actionButtonDanger: (armed: boolean) =>
    `flex h-7 items-center justify-center rounded-md px-2 text-xs font-medium transition-colors ${
      armed
        ? "bg-pg-danger-subtle text-pg-danger-fg"
        : "text-pg-fg-muted hover:bg-pg-danger-subtle hover:text-pg-danger-fg"
    }`,
  footer:
    "flex shrink-0 items-center justify-center gap-x-3 overflow-x-auto bg-pg-canvas-subtle px-3 py-1.5 text-[11px] leading-4 text-pg-fg-muted [scrollbar-width:none] [&::-webkit-scrollbar]:hidden",
  footerHint: "flex shrink-0 items-center gap-1 whitespace-nowrap",
  kbd: "rounded border border-pg-border-subtle bg-pg-canvas-default px-1 font-mono text-[10px] leading-3 text-pg-fg-muted",
};

const SEARCH_WINDOW_FIXED_WIDTH = 780;
const SEARCH_WINDOW_MAX_HEIGHT = 620;
/** 关键词进入查询的防抖：连击/IME 拼写期间不逐键触发 FTS 查询 */
const SEARCH_INPUT_DEBOUNCE_MS = 200;
/** 触底前预加载下一页的距离 */
const SEARCH_LOAD_MORE_ROOT_MARGIN_PX = 240;
const SEARCH_IMAGE_THUMBNAIL_STYLE = {
  width: 36,
  height: 36,
} as const;

/** 类型筛选；标签是独立维度，通过常驻 chip 行组合使用，不占类型下拉位 */
const FILTER_OPTIONS: Array<{ value: SearchQuickFilter; label: string }> = [
  { value: "all", label: "全部" },
  { value: "favorite", label: "收藏" },
  { value: "text", label: "文本" },
  { value: "image", label: "图片" },
  { value: "file", label: "文件" },
];

async function refreshSearchQueries() {
  await Promise.all([
    invalidateClipQueries(),
    queryClient.invalidateQueries({ queryKey: queryKeys.tags }),
  ]);
}

function getEmptyState(
  hasKeyword: boolean,
  activeFilter: SearchQuickFilter,
  openHint: string,
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
    description: openHint,
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
  // 类型由左侧图标表达，meta 行不重复类型文字
  const meta = [
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

async function handleSearchWindowDragStart(event: MouseEvent<HTMLElement>) {
  const target = event.target as HTMLElement | null;
  if (!target || target.closest("input, button, textarea, select, [data-no-window-drag='true']")) {
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
  const { keyword, reset, selectedItemId, session, setKeyword, setSelectedItemId, setSession } =
    useSearchStore();
  const tauriRuntime = isTauriRuntime();
  const shellRef = useRef<HTMLDivElement>(null);
  const headerRef = useRef<HTMLElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const sectionBarRef = useRef<HTMLDivElement>(null);
  const listContentRef = useRef<HTMLDivElement>(null);
  const listScrollRef = useRef<HTMLElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const selectedItemIdRef = useRef<string | null>(selectedItemId);
  const itemRefs = useRef<(HTMLDivElement | null)[]>([]);
  const [inputSuspended, setInputSuspended] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [activeFilter, setActiveFilter] = useState<SearchQuickFilter>("all");
  const [activeTagNames, setActiveTagNames] = useState<string[]>([]);
  // 防抖后的关键词才进查询：输入框展示即时值 keyword，查询与列表切换依据 debouncedKeyword
  const [debouncedKeyword, setDebouncedKeyword] = useState(keyword);
  const isComposingRef = useRef(false);
  const loadMoreRef = useRef<HTMLDivElement | null>(null);
  const footerRef = useRef<HTMLElement>(null);
  const errorTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const resizeFrameRef = useRef<number | null>(null);
  const lastAppliedWindowHeightRef = useRef<number | null>(null);
  // 窗口高度棘轮：结构性变化（关键词/筛选/条目数/会话）时允许收缩重算，
  // 选中态展开等局部变化只允许增长，避免上下键导航时窗口反复抖动
  const allowWindowShrinkRef = useRef(true);
  const favoriteTogglePendingRef = useRef(false);
  // 两段式删除：待确认目标渲染为“再次点击确认删除”，3 秒未确认自动撤销
  const {
    armedTarget: deleteArmedId,
    request: requestArmedDelete,
    reset: resetDeleteArmed,
  } = useArmedConfirm<string>(async (id) => {
    try {
      await deleteItem(id);
      applyClipsChanged({ kind: "deleted", id });
    } catch (error) {
      showError("删除条目失败，请稍后重试");
      console.error("删除条目失败", error);
    }
  });
  const hasKeyword = debouncedKeyword.trim().length > 0;
  const recentQuery = useSearchRecentQuery(activeFilter, activeTagNames, !hasKeyword);
  const searchQuery = useSearchSearchQuery(
    debouncedKeyword,
    activeFilter,
    activeTagNames,
    hasKeyword,
  );
  // 标签 chip 行常驻可组合筛选，不再依赖类型下拉的"标签"选项
  const tagsQuery = useTagsQuery(true);
  const settingsQuery = useSettingsQuery({ staleTime: 0 });
  const activeQuery = hasKeyword ? searchQuery : recentQuery;
  const items = useMemo<ClipItemSummary[]>(
    () => activeQuery.data?.pages.flatMap((page) => page.items) ?? [],
    [activeQuery.data],
  );
  const imageCache = useImageUrlCache(items);
  const itemsRef = useRef<ClipItemSummary[]>(items);
  const restoreClipboardRef = useRef(settingsQuery.data?.restoreClipboardAfterPaste ?? true);
  const detailQuery = useItemDetailQuery(selectedItemId);

  // 搜索窗口仅图片条目提供悬停预览
  const { cancelTooltip, handleMouseMove: handleItemMouseMove } = useHoverTooltip({
    enabled: tauriRuntime,
    themePreset: settingsQuery.data?.themePreset,
    themeAccent: settingsQuery.data?.themeAccent,
    shouldShow: (item) => item.type === "image",
    buildHtml: async (item, requestId) => {
      const imageUrl = await imageCache.resolve(item);
      return buildTooltipHtml(item, { imageUrl, requestId });
    },
  });

  // 清理错误定时器
  useEffect(() => {
    return () => {
      if (errorTimerRef.current) {
        clearTimeout(errorTimerRef.current);
      }
      if (resizeFrameRef.current) {
        cancelAnimationFrame(resizeFrameRef.current);
      }
    };
  }, []);

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

  useEffect(() => {
    itemsRef.current = items;
    selectedItemIdRef.current = selectedItemId;
  }, [items, selectedItemId]);

  // 关键词防抖：IME 组合期间不更新（拼写过程中的每个拼音字母不触发查询），
  // 组合结束后下一帧读取 store 的最终值，随后恢复常规防抖节奏
  useEffect(() => {
    if (isComposingRef.current) {
      return;
    }
    const timer = setTimeout(() => setDebouncedKeyword(keyword), SEARCH_INPUT_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [keyword]);

  // 搜索结果渐进加载：滚动接近底部时预取下一页（最近条目与关键词搜索共用）
  const hasNextPage = activeQuery.hasNextPage;
  const isFetchingNextPage = activeQuery.isFetchingNextPage;
  const fetchNextPage = activeQuery.fetchNextPage;
  useEffect(() => {
    if (!hasNextPage) {
      return;
    }

    const sentinel = loadMoreRef.current;
    if (!sentinel || typeof IntersectionObserver === "undefined") {
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting) && !isFetchingNextPage) {
          void fetchNextPage();
        }
      },
      {
        root: listScrollRef.current,
        rootMargin: `${SEARCH_LOAD_MORE_ROOT_MARGIN_PX}px`,
      },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage, items.length]);

  useEffect(() => {
    restoreClipboardRef.current = settingsQuery.data?.restoreClipboardAfterPaste ?? true;
  }, [settingsQuery.data?.restoreClipboardAfterPaste]);

  // 关键词/筛选/条目数/错误条属于结构性变化，允许下一次高度同步收缩重算
  useEffect(() => {
    allowWindowShrinkRef.current = true;
  }, [debouncedKeyword, activeFilter, activeTagNames, errorMessage, items.length]);

  const handleItemMouseLeave = () => {
    if (!tauriRuntime) {
      return;
    }
    cancelTooltip();
  };

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

  // 选中项变化时，若待确认目标是其他条目则立即撤销
  useEffect(() => {
    if (deleteArmedId && deleteArmedId !== selectedItemId) {
      resetDeleteArmed();
    }
  }, [selectedItemId]);

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
        const shellHeight = Math.round(shellRect?.height ?? 0);
        const headerHeight = headerRef.current?.offsetHeight ?? 0;
        const errorHeight = errorRef.current?.offsetHeight ?? 0;
        const sectionHeight = sectionBarRef.current?.offsetHeight ?? 0;
        const footerHeight = footerRef.current?.offsetHeight ?? 0;
        const listContentHeight = Math.ceil(
          listContentRef.current?.getBoundingClientRect().height ?? 0,
        );

        if (!headerHeight || !sectionHeight || !listContentHeight) {
          return;
        }

        const chromeHeight = 5 + headerHeight + errorHeight + sectionHeight + footerHeight;
        const contentHeight = chromeHeight + listContentHeight;
        const targetHeight = Math.min(SEARCH_WINDOW_MAX_HEIGHT, contentHeight);

        if (lastAppliedWindowHeightRef.current === targetHeight && shellHeight === targetHeight) {
          return;
        }

        // 非结构性变化（如选中态展开）只允许窗口增高，避免键盘导航时高度反复抖动
        if (
          !allowWindowShrinkRef.current &&
          lastAppliedWindowHeightRef.current !== null &&
          targetHeight < lastAppliedWindowHeightRef.current
        ) {
          return;
        }

        allowWindowShrinkRef.current = false;
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
      footerRef.current,
      listContentRef.current,
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

  // 标签芯片多选（AND 语义）；按忽略大小写比对，与后端 NOCASE 行为一致
  const toggleActiveTag = (tagName: string) => {
    setActiveTagNames((current) =>
      current.some((name) => name.toLowerCase() === tagName.toLowerCase())
        ? current.filter((name) => name.toLowerCase() !== tagName.toLowerCase())
        : [...current, tagName],
    );
  };

  const clearKeyword = () => {
    setKeyword("");
    setDebouncedKeyword("");
    searchInputRef.current?.focus();
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

  const handleEventListenError = (error: unknown) => {
    console.error("注册搜索窗口事件监听失败", error);
    showError("搜索窗口初始化失败，部分快捷操作可能不可用");
  };

  useAppEvent<{
    source: string;
    itemId?: string;
    initialKeyword?: string;
  }>(
    SEARCH_SESSION_START_EVENT,
    async (payload) => {
      await invalidateSettings(queryClient);

      const initialKeyword = payload.initialKeyword ?? "";
      setSession({
        source: "global" as const,
        initialItemId: payload.itemId,
        initialKeyword: payload.initialKeyword,
      } as SearchSession);
      setActiveFilter("all");
      setActiveTagNames([]);
      setKeyword(initialKeyword);
      // 会话初始关键词直接生效，不经防抖
      setDebouncedKeyword(initialKeyword);
      setSelectedItemId(payload.itemId ?? null);
      setInputSuspended(false);
      // 新会话重新按内容自适应高度
      allowWindowShrinkRef.current = true;
      lastAppliedWindowHeightRef.current = null;
      // 窗口通过 hide/show 复用，DOM 滚动位置会被保留。
      // 每次打开都把列表滚回顶部，避免停留在上次关闭时的位置。
      listScrollRef.current?.scrollTo({ top: 0 });
    },
    handleEventListenError,
  );

  useAppEvent<ClipsChangedPayload>(
    CLIPS_CHANGED_EVENT,
    (payload) => {
      applyClipsChanged(payload);
    },
    handleEventListenError,
  );

  useAppEvent(
    SEARCH_SESSION_END_EVENT,
    () => {
      setInputSuspended(false);
      setActiveFilter("all");
      setActiveTagNames([]);
      reset();
      setDebouncedKeyword("");
    },
    handleEventListenError,
  );

  useAppEvent<string>(
    SEARCH_NAVIGATE_EVENT,
    (direction) => {
      const currentItems = itemsRef.current;
      if (!currentItems.length) {
        return;
      }

      const nextIndex = getNextSearchNavigationIndex(
        currentItems,
        selectedItemIdRef.current,
        direction === "up" ? "up" : "down",
      );

      if (nextIndex >= 0) {
        setSelectedItemId(currentItems[nextIndex]?.id ?? null);
      }
    },
    handleEventListenError,
  );

  useAppEvent(
    SEARCH_EDIT_ITEM_EVENT,
    () => {
      void handleOpenEditor();
    },
    handleEventListenError,
  );

  useAppEvent(
    SEARCH_PASTE_EVENT,
    () => {
      void handlePasteSelected();
    },
    handleEventListenError,
  );

  useAppEvent(
    SEARCH_INPUT_SUSPEND_EVENT,
    () => {
      setInputSuspended(true);
      searchInputRef.current?.blur();
    },
    handleEventListenError,
  );

  useAppEvent(
    SEARCH_INPUT_RESUME_EVENT,
    () => {
      setInputSuspended(false);
      searchInputRef.current?.focus();
    },
    handleEventListenError,
  );

  useAppEvent(
    SETTINGS_CHANGED_EVENT,
    async () => {
      await invalidateSettings(queryClient);
    },
    handleEventListenError,
  );

  useAppEvent(
    TAGS_CHANGED_EVENT,
    () => {
      void refreshSearchQueries().catch((error) => {
        console.error("刷新标签数据失败", error);
      });
    },
    handleEventListenError,
  );

  // handler 内的选中项/列表读取全部走 ref，依赖只需覆盖直接读取的挂起与筛选状态；
  // 缩减依赖避免每次输入/导航都重挂 window 监听
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
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
            const currentItem = itemsRef.current.find(
              (item) => item.id === selectedItemIdRef.current,
            );
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
        case "delete-item":
          void handleDeleteSelected();
          return;
        case "close":
          void handleClose();
          return;
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [inputSuspended]);

  // 两段式删除：首次触发进入待确认态，3 秒内再次触发同一目标才真正删除
  function handleDeleteSelected() {
    const id = selectedItemIdRef.current;
    if (!id) {
      return;
    }

    requestArmedDelete(id);
  }

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

    cancelTooltip();
    try {
      await openEditorFromSearch(currentItem.id);
    } catch (error) {
      showError("打开编辑窗口失败，请稍后重试");
      console.error("打开编辑窗口失败", error);
    }
  }

  async function handlePasteItem(item: ClipItemSummary) {
    try {
      await pasteItem(item.id, {
        restoreClipboardAfterPaste: restoreClipboardRef.current,
        pasteToTarget: true,
      });
    } catch (error) {
      showError("执行粘贴失败，请稍后重试");
      console.error("执行粘贴失败", error);
    }
  }

  async function handlePasteSelected() {
    const currentItem = itemsRef.current.find((item) => item.id === selectedItemIdRef.current);
    if (!currentItem) {
      return;
    }

    await handlePasteItem(currentItem);
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
      queryClient.setQueryData<ClipItemDetail | undefined>(["detail", id], (detail) =>
        setFavoritedOnDetail(detail, id, nextFavorited),
      );
      // search-recent 为分页缓存（InfiniteData），逐页应用同一乐观更新
      queryClient.setQueriesData<InfiniteData<SearchResult>>(
        { queryKey: ["search-recent"] },
        (data) =>
          data
            ? {
                ...data,
                pages: data.pages.map(
                  (page) => setFavoritedOnSearchResult(page, id, nextFavorited) ?? page,
                ),
              }
            : data,
      );
      // search-query 为分页缓存（InfiniteData），逐页应用同一乐观更新
      queryClient.setQueriesData<InfiniteData<SearchResult>>(
        { queryKey: ["search-query"] },
        (data) =>
          data
            ? {
                ...data,
                pages: data.pages.map(
                  (page) => setFavoritedOnSearchResult(page, id, nextFavorited) ?? page,
                ),
              }
            : data,
      );
      if (!nextFavorited && activeFilter === "favorite") {
        const activeQueryKey = hasKeyword
          ? createSearchSearchQueryKey(debouncedKeyword, activeFilter, activeTagNames)
          : createSearchRecentQueryKey(activeFilter, activeTagNames);
        queryClient.setQueryData(
          activeQueryKey,
          (data: SearchResult | InfiniteData<SearchResult> | undefined) => {
            if (!data) {
              return data;
            }
            if ("pages" in data) {
              return {
                ...data,
                pages: data.pages.map(
                  (page) =>
                    setFavoritedOnSearchResult(page, id, nextFavorited, {
                      removeUnfavoritedItem: true,
                    }) ?? page,
                ),
              };
            }
            return (
              setFavoritedOnSearchResult(data, id, nextFavorited, {
                removeUnfavoritedItem: true,
              }) ?? data
            );
          },
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

  const activeQueryIsLoading = activeQuery.isLoading;
  // 加载失败且没有可展示的缓存数据时给错误态；有旧数据时继续展示列表
  const loadError = activeQuery.isError && items.length === 0 ? activeQuery.error : null;
  const totalCount = activeQuery.data?.pages[0]?.total ?? items.length;
  const resultCountLabel = `${items.length}/${totalCount} 条`;
  const searchOpenShortcut = settingsQuery.data?.searchShortcutEnabled
    ? settingsQuery.data.searchShortcut
    : null;
  const emptyState = getEmptyState(
    hasKeyword,
    activeFilter,
    searchOpenShortcut
      ? `复制内容后使用 ${searchOpenShortcut} 打开此窗口`
      : "复制内容后即可在此查看",
  );

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
          <div className={STYLES.searchControl(inputSuspended)} data-no-window-drag="true">
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
              onCompositionEnd={() => {
                isComposingRef.current = false;
                // Chrome 中 compositionend 先于最后一次 input 事件，延后一帧再取最终值
                requestAnimationFrame(() => {
                  setDebouncedKeyword(useSearchStore.getState().keyword);
                });
              }}
              onCompositionStart={() => {
                isComposingRef.current = true;
              }}
              placeholder={inputSuspended ? "键盘已交给速贴面板，按 Esc 返回" : "开始键入..."}
              aria-label="搜索剪贴板记录"
              value={keyword}
            />
            {keyword ? (
              <button
                aria-label="清除搜索关键词"
                className={STYLES.searchClearButton}
                onMouseDown={(event) => event.preventDefault()}
                onClick={clearKeyword}
                title="清除"
                type="button"
              >
                <svg
                  aria-hidden="true"
                  className="h-4 w-4"
                  fill="none"
                  stroke="currentColor"
                  strokeLinecap="round"
                  strokeWidth="1.8"
                  viewBox="0 0 24 24"
                >
                  <path d="M6 6l12 12M18 6 6 18" />
                </svg>
              </button>
            ) : null}
            <span aria-hidden="true" className={STYLES.searchFilterDivider} />
            {activeQueryIsLoading ? null : (
              <span className={STYLES.resultCount}>{resultCountLabel}</span>
            )}
          </div>
        </header>

        {errorMessage ? (
          <div
            ref={errorRef}
            className="border-b border-pg-danger-fg/20 bg-pg-danger-subtle px-5 py-2 text-sm text-pg-danger-fg"
            role="alert"
          >
            {errorMessage}
          </div>
        ) : null}

        <div ref={sectionBarRef}>
          <div className={STYLES.filterRow} data-no-window-drag="true">
            {FILTER_OPTIONS.map((option) => (
              <button
                aria-pressed={activeFilter === option.value}
                className={STYLES.filterChip(activeFilter === option.value)}
                key={option.value}
                onClick={() => setActiveFilter(option.value)}
                type="button"
              >
                {option.label}
              </button>
            ))}
            {(tagsQuery.data ?? []).length > 0 ? (
              <>
                <div aria-hidden="true" className={STYLES.filterRowDivider} />
                {(tagsQuery.data ?? []).map((tag) => {
                  const selected = activeTagNames.some(
                    (name) => name.toLowerCase() === tag.name.toLowerCase(),
                  );
                  return (
                    <button
                      aria-pressed={selected}
                      className={STYLES.tagChip(selected)}
                      key={tag.name}
                      onClick={() => toggleActiveTag(tag.name)}
                      type="button"
                    >
                      {tag.name}
                    </button>
                  );
                })}
              </>
            ) : null}
          </div>
        </div>

        <main
          ref={listScrollRef}
          className="min-h-0 flex-1 overflow-y-auto [scrollbar-gutter:stable_both-edges]"
        >
          <div ref={listContentRef} className="px-0.5 pb-1 pt-1.5">
            {activeQueryIsLoading ? (
              <div className="flex min-h-[160px] items-center justify-center py-12">
                <LoadingSpinner size="sm" text="加载中..." />
              </div>
            ) : loadError ? (
              <div className="flex min-h-[160px] flex-col items-center justify-center gap-1 px-4 py-12 text-center text-sm text-pg-fg-subtle">
                <span>记录加载失败</span>
                <span className="text-xs">{getErrorMessage(loadError, "请稍后重试")}</span>
                <button
                  className="mt-2 rounded-md border border-pg-border-default px-2.5 py-1 text-xs text-pg-fg-muted transition-colors hover:bg-pg-canvas-subtle"
                  onClick={() => void activeQuery.refetch()}
                  type="button"
                >
                  重试
                </button>
              </div>
            ) : items.length === 0 ? (
              <div className="flex min-h-[160px] flex-col items-center justify-center gap-1 px-4 py-12 text-center text-sm text-pg-fg-subtle">
                <span>{emptyState.title}</span>
                <span>{emptyState.description}</span>
                {hasKeyword ? (
                  <button
                    className="mt-2 rounded-md border border-pg-border-default px-2.5 py-1 text-xs text-pg-fg-muted transition-colors hover:bg-pg-canvas-subtle"
                    onClick={() => {
                      setKeyword("");
                      setDebouncedKeyword("");
                    }}
                    type="button"
                  >
                    清除关键词
                  </button>
                ) : activeFilter !== "all" || activeTagNames.length > 0 ? (
                  <button
                    className="mt-2 rounded-md border border-pg-border-default px-2.5 py-1 text-xs text-pg-fg-muted transition-colors hover:bg-pg-canvas-subtle"
                    onClick={() => {
                      setActiveFilter("all");
                      setActiveTagNames([]);
                    }}
                    type="button"
                  >
                    清除筛选
                  </button>
                ) : null}
              </div>
            ) : (
              <div
                aria-activedescendant={selectedItemId ? `search-item-${selectedItemId}` : undefined}
                aria-label="剪贴板记录列表"
                className="px-1 pb-1 pt-0.5"
                role="listbox"
              >
                {items.map((item, index) => {
                  const isSelected = selectedItemId === item.id;
                  const inlineDetail = isSelected ? (detailQuery.data ?? item) : null;
                  const isFavorited = getSearchItemFavoritedState(
                    item,
                    detailQuery.data?.id === item.id ? detailQuery.data : null,
                  );
                  const imageUrl = item.type === "image" ? imageCache.getCached(item.id) : null;
                  const itemMeta = getItemDetailMeta(inlineDetail ?? item);
                  const selectedPreviewText = detailQuery.isLoading
                    ? "正在载入条目详情..."
                    : detailQuery.data?.type === "text"
                      ? detailQuery.data.fullText || detailQuery.data.contentPreview
                      : item.contentPreview;
                  const previewText = isSelected ? selectedPreviewText : item.contentPreview;

                  return (
                    <div
                      aria-selected={isSelected}
                      className={STYLES.listItemShell(isSelected)}
                      id={`search-item-${item.id}`}
                      key={item.id}
                      ref={(el) => {
                        itemRefs.current[index] = el;
                      }}
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
                        void handlePasteItem(item);
                      }}
                      role="option"
                      tabIndex={-1}
                    >
                      <div className={STYLES.listItemLayout()}>
                        <div>
                          {imageUrl ? (
                            <img
                              alt=""
                              className={STYLES.imageThumb(isSelected)}
                              decoding="async"
                              loading="lazy"
                              onError={() => {
                                imageCache.markError(item.id);
                              }}
                              src={imageUrl}
                              style={SEARCH_IMAGE_THUMBNAIL_STYLE}
                            />
                          ) : (
                            <div className={STYLES.glyphBox(isSelected)}>
                              <ClipTypeIcon className={STYLES.glyphIcon} type={item.type} />
                            </div>
                          )}
                        </div>
                        <div className="min-w-0">
                          <div className="flex min-w-0 items-start gap-2">
                            <p
                              className={`min-w-0 flex-1 text-[15px] leading-6 ${
                                isSelected
                                  ? "line-clamp-3 whitespace-pre-wrap break-words pr-[148px] font-medium text-pg-fg-default"
                                  : "truncate pr-1 text-pg-fg-default"
                              }`}
                            >
                              {previewText}
                            </p>
                            {isFavorited && !isSelected ? (
                              <span
                                aria-label="已收藏"
                                className="shrink-0 text-[12px] text-pg-favorite"
                                role="img"
                              >
                                ★
                              </span>
                            ) : null}
                          </div>
                          <div className={STYLES.inlineMetaRow}>
                            {itemMeta.map((meta, metaIndex) => (
                              <span key={`${item.id}-${meta}`}>
                                {metaIndex > 0 ? <span aria-hidden="true">• </span> : null}
                                {meta}
                              </span>
                            ))}
                          </div>
                          {item.tags.length > 0 ? (
                            <div className={STYLES.itemTagChips}>
                              {item.tags.slice(0, 3).map((tagName) => (
                                <span className={STYLES.itemTagChip} key={tagName}>
                                  {tagName}
                                </span>
                              ))}
                              {item.tags.length > 3 ? (
                                <span className={STYLES.itemTagOverflow}>
                                  +{item.tags.length - 3}
                                </span>
                              ) : null}
                            </div>
                          ) : null}
                          {isSelected && item.type === "image" && imageUrl ? (
                            <img
                              alt=""
                              className={STYLES.imagePreviewLarge}
                              decoding="async"
                              src={imageUrl}
                            />
                          ) : null}
                        </div>
                      </div>
                      {isSelected ? (
                        <div
                          className={STYLES.selectedActions}
                          onClick={(event) => event.stopPropagation()}
                          onDoubleClick={(event) => event.stopPropagation()}
                          onMouseMove={(event) => {
                            event.stopPropagation();
                            cancelTooltip();
                          }}
                        >
                          <button
                            aria-label="粘贴当前条目"
                            className={STYLES.actionButton}
                            onMouseDown={(event) => {
                              event.preventDefault();
                              event.stopPropagation();
                            }}
                            onClick={() => void handlePasteItem(item)}
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
                          <button
                            aria-label={
                              deleteArmedId === item.id ? "确认删除当前条目" : "删除当前条目"
                            }
                            className={STYLES.actionButtonDanger(deleteArmedId === item.id)}
                            onMouseDown={(event) => {
                              event.preventDefault();
                              event.stopPropagation();
                            }}
                            onClick={() => void handleDeleteSelected()}
                            title={deleteArmedId === item.id ? "再次点击确认删除" : "删除"}
                            type="button"
                          >
                            {deleteArmedId === item.id ? (
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
                                <path d="M9.5 4.75h5V7h-5z" />
                                <path d="M6.5 7l.8 12.25h9.4L17.5 7" />
                                <path d="M10 10.5v6M14 10.5v6" />
                              </svg>
                            )}
                          </button>
                        </div>
                      ) : (
                        <button
                          aria-label={`粘贴：${item.contentPreview.slice(0, 20)}`}
                          className={STYLES.hoverPasteButton}
                          onMouseDown={(event) => {
                            event.preventDefault();
                            event.stopPropagation();
                          }}
                          onClick={(event) => {
                            event.stopPropagation();
                            void handlePasteItem(item);
                          }}
                          title="粘贴"
                          type="button"
                        >
                          <svg
                            aria-hidden="true"
                            className="h-3.5 w-3.5"
                            fill="none"
                            stroke="currentColor"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                            strokeWidth="1.8"
                            viewBox="0 0 24 24"
                          >
                            <path d="M9.75 3h4.5A1.75 1.75 0 0 1 16 4.75V6H8V4.75A1.75 1.75 0 0 1 9.75 3Z" />
                            <rect x="5" y="6" width="14" height="15" rx="2.75" />
                          </svg>
                          粘贴
                        </button>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
            {hasNextPage ? (
              <div
                className="flex items-center justify-center py-3 text-xs text-pg-fg-subtle"
                ref={loadMoreRef}
              >
                {isFetchingNextPage ? "正在加载更多..." : ""}
              </div>
            ) : null}
          </div>
        </main>
        <footer className={STYLES.footer} ref={footerRef}>
          {inputSuspended ? (
            <>
              <span className={STYLES.footerHint}>
                <kbd className={STYLES.kbd}>↑↓</kbd>选择
              </span>
              <span className={STYLES.footerHint}>
                <kbd className={STYLES.kbd}>Enter</kbd>粘贴
              </span>
              <span className={STYLES.footerHint}>
                <kbd className={STYLES.kbd}>1-9</kbd>直达
              </span>
              <span className={STYLES.footerHint}>
                <kbd className={STYLES.kbd}>Esc</kbd>返回搜索
              </span>
            </>
          ) : (
            <>
              <span className={STYLES.footerHint}>
                <kbd className={STYLES.kbd}>↑↓</kbd>选择
              </span>
              <span className={STYLES.footerHint}>
                <kbd className={STYLES.kbd}>Enter</kbd>粘贴
              </span>
              <span className={STYLES.footerHint}>
                <kbd className={STYLES.kbd}>Ctrl+Enter</kbd>编辑
              </span>
              <span className={STYLES.footerHint}>
                <kbd className={STYLES.kbd}>Del</kbd>删除
              </span>
              <span className={STYLES.footerHint}>
                <kbd className={STYLES.kbd}>Esc</kbd>关闭
              </span>
            </>
          )}
        </footer>
      </div>
    </div>
  );
}
