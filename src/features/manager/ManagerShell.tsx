import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Panel } from "../../shared/ui/Panel";
import { StatusBadge } from "../../shared/ui/StatusBadge";
import { EmptyState } from "../../shared/components/EmptyState";
import { LoadingSpinner } from "../../shared/ui/LoadingSpinner";
import { showPicker } from "../../bridge/commands";
import {
  CLIPS_CHANGED_EVENT,
  MANAGER_OPEN_SETTINGS_EVENT,
  SETTINGS_CHANGED_EVENT,
} from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import { hideCurrentWindow } from "../../bridge/window";
import { queryClient } from "../../app/queryClient";
import type { ClipItemDetail, SearchSort } from "../../shared/types/clips";
import type { PickerPositionMode, ThemeMode, UserSetting } from "../../shared/types/settings";
import {
  getClipTypeIcon,
  getClipTypeLabel,
  getFileCountLabel,
} from "../../shared/utils/clipDisplay";
import { formatDateTime } from "../../shared/utils/time";
import { useManagerStore } from "./store";
import {
  useDeleteItemMutation,
  useFavoritesQuery,
  useItemDetailQuery,
  usePasteMutation,
  useSearchQuery,
  useSetFavoritedMutation,
  useSettingsQuery,
  useUpdateSettingsMutation,
  useUpdateTextMutation,
} from "./queries";

// --- 样式常量抽象 ---
const STYLES = {
   logoBadge: "inline-flex items-center gap-2 rounded-full border border-[color:var(--cp-accent-primary)]/20 bg-[color:var(--cp-accent-primary)]/10 pl-1.5 pr-3 py-1 text-[10px] font-bold uppercase tracking-[0.2em] text-[color:var(--cp-accent-primary)] shadow-none",
   logoIcon: "flex h-5 w-5 items-center justify-center rounded-full bg-[color:var(--cp-accent-primary)] text-cp-base shadow-none",
    shortcutCard: "relative shrink-0 overflow-hidden rounded-lg bg-[color:var(--cp-card-surface)] px-6 py-6 text-[color:var(--cp-text-primary)] shadow-xs ring-1 ring-[color:var(--cp-border-soft)] transition-all duration-300 hover:ring-[rgba(var(--cp-peach-rgb),0.2)] dark:bg-[color:var(--cp-card-surface)]/30 dark:shadow-none",
    shortcutDot: "h-2 w-2 rounded-full bg-[color:var(--cp-accent-primary)] shadow-sm ring-2 ring-[rgba(var(--cp-peach-rgb),0.2)]",
    favoriteItem: "group w-full rounded-md border border-[color:var(--cp-border-weak)] bg-cp-mantle/50 px-4 py-3 text-left transition-all duration-300 hover:border-[rgba(var(--cp-peach-rgb),0.25)] hover:shadow-sm hover:bg-cp-mantle dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[rgba(var(--cp-surface0-rgb),0.4)]",
    primaryButton: "group relative flex w-full items-center justify-center gap-2 rounded-md bg-[color:var(--cp-accent-primary)] px-4 py-3.5 text-sm font-bold text-cp-base shadow-sm transition-all duration-300 hover:bg-[color:var(--cp-accent-primary-strong)] active:translate-y-0",
    viewModeToggle: (active: boolean) => `flex items-center justify-center gap-2 whitespace-nowrap rounded-md px-4 py-2.5 text-sm font-bold transition-all duration-300 ${active
      ? "bg-gradient-to-r from-[rgba(var(--cp-peach-rgb),0.15)] to-[rgba(var(--cp-peach-rgb),0.08)] text-[color:var(--cp-accent-primary-strong)] shadow-none ring-1 ring-[rgba(var(--cp-peach-rgb),0.3)]"
     : "text-[color:var(--cp-text-secondary)] hover:text-[color:var(--cp-text-primary)] hover:bg-[color:var(--cp-control-surface-hover)]/40"
     }`,
    searchInput: "w-full rounded-md border border-[color:var(--cp-border-weak)] bg-cp-mantle py-3 pl-11 pr-5 text-sm outline-none backdrop-blur-sm transition-all duration-300 placeholder:text-[color:var(--cp-text-muted)] focus:border-[rgba(var(--cp-peach-rgb),0.35)] focus:bg-[color:var(--cp-window-shell)] focus:shadow-sm focus:shadow-[rgba(var(--cp-peach-rgb),0.08)] focus-visible:outline-none dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]",
    historyItem: (selected: boolean) => `group w-full rounded-md border px-6 py-5 text-left transition-all duration-300 ${selected
     ? "relative z-10 scale-[1.01] border-[rgba(var(--cp-peach-rgb),0.4)] bg-gradient-to-br from-cp-mantle to-[rgba(var(--cp-peach-rgb),0.05)] shadow-sm shadow-[rgba(var(--cp-peach-rgb),0.08)] ring-1 ring-[rgba(var(--cp-peach-rgb),0.2)] dark:border-[rgba(var(--cp-peach-rgb),0.35)] dark:bg-gradient-to-br dark:from-[rgba(var(--cp-surface0-rgb),0.6)] dark:to-[rgba(var(--cp-peach-rgb),0.08)] dark:shadow-none"
     : "border-[color:var(--cp-border-weak)] bg-cp-mantle/30 hover:border-[rgba(var(--cp-peach-rgb),0.2)] hover:bg-cp-mantle/60 dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
     }`,
    detailEditor: "min-h-[200px] flex-1 w-full resize-none rounded-md border border-[color:var(--cp-border-weak)] bg-cp-mantle px-5 py-5 text-[14px] leading-relaxed outline-none transition-all duration-300 focus:border-[rgba(var(--cp-peach-rgb),0.35)] focus:bg-[color:var(--cp-window-shell)] focus:shadow-sm focus:shadow-[rgba(var(--cp-peach-rgb),0.08)] focus-visible:outline-none dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]",
};

const pickerPositionOptions: Array<{
  value: PickerPositionMode;
  label: string;
  description: string;
}> = [
    {
      value: "mouse",
      label: "鼠标位置",
      description: "默认推荐，速贴窗口会贴近当前鼠标所在位置弹出。",
    },
    {
      value: "lastPosition",
      label: "上次关闭时的位置",
      description: "保留上次拖动或关闭时的位置；首次使用会落在屏幕中心。",
    },
    {
      value: "caret",
      label: "光标所在位置",
      description: "优先跟随当前输入光标；如果系统拿不到光标位置，会退回鼠标位置。",
    },
  ];

const themeModeOptions: Array<{
  value: ThemeMode;
  label: string;
  description: string;
}> = [
    {
      value: "system",
      label: "跟随系统",
      description: "自动匹配 Windows 当前的浅色或深色外观。",
    },
    {
      value: "light",
      label: "浅色",
      description: "温润低饱和度的 Latte 主题，适合日常办公。",
    },
    {
      value: "dark",
      label: "深色",
      description: "温暖柔和的 Macchiato 主题，适合夜间使用。",
    },
  ];

const MANAGER_PAGE_SIZE = 50;
const SEARCH_DEBOUNCE_MS = 250;

export function ManagerShell() {
  const { selectedItemId, draftText, viewMode, setDraftText, setSelectedItemId, setViewMode } =
    useManagerStore();
  const [keyword, setKeyword] = useState("");
  const [debouncedKeyword, setDebouncedKeyword] = useState("");
  const [favoritedOnly, setFavoritedOnly] = useState(false);
  const [pageIndex, setPageIndex] = useState(0);

  useEffect(() => {
    const timeoutId = window.setTimeout(() => {
      setDebouncedKeyword(keyword);
    }, SEARCH_DEBOUNCE_MS);

    return () => window.clearTimeout(timeoutId);
  }, [keyword]);

  useEffect(() => {
    setPageIndex(0);
    setSelectedItemId(null);
  }, [debouncedKeyword, favoritedOnly, setSelectedItemId]);

  const searchQuery = useMemo(
    () => ({
      keyword: debouncedKeyword,
      filters: {
        favoritedOnly,
      },
      offset: pageIndex * MANAGER_PAGE_SIZE,
      limit: MANAGER_PAGE_SIZE,
      sort: (debouncedKeyword.trim() ? "relevance_desc" : "recent_desc") as SearchSort,
    }),
    [debouncedKeyword, favoritedOnly, pageIndex],
  );

  const favorites = useFavoritesQuery();
  const clips = useSearchQuery(searchQuery);
  const detail = useItemDetailQuery(selectedItemId);
  const settings = useSettingsQuery();
  const updateTextMutation = useUpdateTextMutation();
  const deleteMutation = useDeleteItemMutation();
  const favoritedMutation = useSetFavoritedMutation();
  const pasteMutation = usePasteMutation();
  const updateSettingsMutation = useUpdateSettingsMutation();

  useEffect(() => {
    if (!clips.data) {
      return;
    }

    const maxPageIndex = Math.max(Math.ceil(clips.data.total / MANAGER_PAGE_SIZE) - 1, 0);
    if (pageIndex > maxPageIndex) {
      setPageIndex(maxPageIndex);
    }
  }, [clips.data, pageIndex]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    let offClips: (() => void) | undefined;
    let offSettings: (() => void) | undefined;
    let offOpenSettings: (() => void) | undefined;

    void listen(CLIPS_CHANGED_EVENT, async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["favorites"] }),
        queryClient.invalidateQueries({ queryKey: ["search"] }),
        queryClient.invalidateQueries({ queryKey: ["detail"] }),
      ]);
    }).then((cleanup) => {
      offClips = cleanup;
    });

    void listen(SETTINGS_CHANGED_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }).then((cleanup) => {
      offSettings = cleanup;
    });

    void listen(MANAGER_OPEN_SETTINGS_EVENT, async () => {
      setViewMode("settings");
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }).then((cleanup) => {
      offOpenSettings = cleanup;
    });

    return () => {
      offClips?.();
      offSettings?.();
      offOpenSettings?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void hideCurrentWindow().catch(console.error);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  useEffect(() => {
    if (detail.data && detail.data.type === "text") {
      setDraftText(detail.data.fullText ?? "");
    }
  }, [detail.data, setDraftText]);

  const selectedSummary = clips.data?.items.find((item) => item.id === selectedItemId) ?? detail.data;
  const settingsSaveError = updateSettingsMutation.error
    ? getErrorMessage(updateSettingsMutation.error)
    : null;
  const currentItems = clips.data?.items ?? [];
  const totalCount = clips.data?.total ?? 0;
  const pageStart = totalCount === 0 ? 0 : searchQuery.offset + 1;
  const pageEnd = totalCount === 0 ? 0 : searchQuery.offset + currentItems.length;
  const hasPreviousPage = pageIndex > 0;
  const hasNextPage = pageEnd < totalCount;

  // 格式化字节大小
  const formatBytes = (bytes?: number | null): string => {
    if (!bytes) return "未知";
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
  };

  return (
    <main className="flex min-h-screen flex-col overflow-y-auto px-4 py-6 text-ink lg:h-screen lg:overflow-hidden md:px-6">
      <div className="mx-auto grid w-full max-w-[1600px] flex-1 min-h-0 gap-4 xl:grid-cols-[280px_minmax(360px,1fr)_420px] lg:grid-cols-[260px_minmax(320px,1fr)_380px]">
        <Panel className="flex flex-col gap-5 lg:overflow-hidden min-h-[600px] lg:min-h-0">
          <div className="shrink-0">
            <div className={STYLES.logoBadge}>
              <span className={STYLES.logoIcon}>
                <svg className="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
                </svg>
              </span>
              FloatPaste
            </div>
            <h1 className="mt-4 font-display text-[2rem] font-medium tracking-tight text-[color:var(--cp-text-primary)]">资料库窗口</h1>
            <p className="mt-3 text-[13px] leading-relaxed text-[color:var(--cp-text-secondary)]">
              管理文本、图片、文件的剪贴记录，支持搜索、收藏与快捷粘贴。
            </p>
          </div>

          <div className="flex shrink-0 flex-wrap gap-2">
            <StatusBadge tone={settings.data?.pauseMonitoring ? "paused" : "running"}>
              {settings.data?.pauseMonitoring ? "监听已暂停" : "监听中"}
            </StatusBadge>
            <StatusBadge tone="muted">
              {`收藏 ${favorites.data?.length ?? 0}`}
            </StatusBadge>
          </div>

          <div className={STYLES.shortcutCard}>
            <div className="relative z-10">
              <div className="flex items-center gap-2">
                <div className={STYLES.shortcutDot}></div>
                <p className="text-[10px] font-bold uppercase tracking-[0.25em] text-[color:var(--cp-text-muted)]">全局快捷键</p>
              </div>
              <p className="mt-2.5 font-display text-3xl font-medium tracking-wide text-[color:var(--cp-text-primary)]">{settings.data?.shortcut ?? "Ctrl+`"}</p>
              <p className="mt-3 text-xs leading-relaxed text-[color:var(--cp-text-muted)] font-medium">唤起速贴面板进行剪贴板管理</p>
            </div>
          </div>

          <div className="flex min-h-[200px] lg:min-h-0 flex-1 flex-col">
            <div className="mb-3 flex shrink-0 items-center justify-between">
              <h2 className="text-[11px] font-bold uppercase tracking-[0.18em] text-[color:var(--cp-text-muted)]">
                收藏预览
              </h2>
              <button
                className="rounded-lg px-2 py-1 text-[13px] font-semibold text-[color:var(--cp-accent-primary)] transition-all hover:bg-[color:var(--cp-accent-primary)]/10 active:scale-95"
                onClick={() => setViewMode("history")}
                type="button"
              >
                查看全部
              </button>
            </div>
            <div className="flex-1 space-y-3 overflow-y-auto pr-2">
              {favorites.data?.length ? (
                favorites.data.map((item) => (
                  <button
                    className={STYLES.favoriteItem}
                    key={item.id}
                    onClick={() => {
                      setViewMode("history");
                      setSelectedItemId(item.id);
                    }}
                    type="button"
                  >
                    <p
                      className="line-clamp-3 text-[12px] leading-relaxed text-[color:var(--cp-text-secondary)] whitespace-pre-wrap break-words [overflow-wrap:anywhere] transition-colors group-hover:text-[color:var(--cp-text-primary)]"
                      title={item.tooltipText || item.contentPreview}
                    >
                      {item.contentPreview}
                    </p>
                    <p className="mt-2 text-[10px] font-medium text-[color:var(--cp-text-muted)] opacity-80">{formatDateTime(item.lastUsedAt ?? item.createdAt)}</p>
                  </button>
                ))
              ) : (
                <EmptyState title="还没有收藏内容" description="在中间列表里点亮星标后，这里会优先显示常用片段。" />
              )}
            </div>
          </div>

          <div className="mt-auto shrink-0 pt-6">
            <div className="flex flex-col gap-3">
              <button
                className={STYLES.primaryButton}
                onClick={() => void showPicker()}
                type="button"
              >
                <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M14 5l7 7m0 0l-7 7m7-7H3" />
                </svg> 打开速贴面板
              </button>
              <div className="grid grid-cols-2 gap-2 rounded-md bg-[color:var(--cp-control-surface)]/40 p-1.5 backdrop-blur-sm ring-1 ring-[rgba(var(--cp-surface1-rgb),0.2)]">
                <button
                  className={STYLES.viewModeToggle(viewMode === "history")}
                  onClick={() => setViewMode("history")}
                  type="button"
                >
                  <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.5} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                  </svg>
                  历史库
                </button>
                <button
                  className={STYLES.viewModeToggle(viewMode === "settings")}
                  onClick={() => setViewMode("settings")}
                  type="button"
                >
                  <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.5} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.5} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                  </svg>
                  设置
                </button>
              </div>
            </div>
          </div>
        </Panel>

        <Panel className="flex flex-col gap-4 lg:overflow-hidden min-h-[500px] lg:min-h-0">
          {viewMode === "history" ? (
            <>
              <div className="flex shrink-0 flex-col gap-2.5 sm:flex-row">
                <div className="relative flex-1 group">
                  <div className="absolute inset-y-0 left-0 flex items-center pl-4 text-[color:var(--cp-text-muted)] transition-colors pointer-events-none group-focus-within:text-[color:var(--cp-accent-primary-strong)]">
                    <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                    </svg>
                  </div>
                  <input
                    className={STYLES.searchInput}
                    onChange={(event) => setKeyword(event.target.value)}
                    placeholder="搜索全文、来源应用或关键短语"
                    value={keyword}
                  />
                </div>
                <label className="inline-flex cursor-pointer items-center gap-2 rounded-md border border-[color:var(--cp-border-weak)] bg-cp-mantle px-5 py-3 text-sm font-semibold text-[color:var(--cp-text-secondary)] transition-all duration-300 hover:border-[color:var(--cp-border-weak)] hover:bg-cp-mantle/80 hover:text-[color:var(--cp-text-primary)] active:scale-95">
                  <input
                    className="h-4 w-4 rounded border-[color:var(--cp-border-strong)] bg-cp-base text-[color:var(--cp-accent-primary)] focus:ring-[color:var(--cp-accent-primary)]"
                    checked={favoritedOnly}
                    onChange={(event) => setFavoritedOnly(event.target.checked)}
                    type="checkbox"
                  />
                  只看收藏
                </label>
              </div>

              <div className="flex shrink-0 items-center justify-between px-1 text-[11px] font-bold uppercase tracking-wider text-[color:var(--cp-text-muted)]">
                <span>共 {totalCount} 条记录</span>
                <span>{debouncedKeyword.trim() ? "按相关度排序" : "按最近活跃排序"}</span>
              </div>

              <div className="flex-1 space-y-3 overflow-y-auto pr-2 pl-1 py-1">
                {clips.isLoading ? (
                  <div className="flex h-full items-center justify-center">
                    <LoadingSpinner text="加载历史记录..." />
                  </div>
                ) : currentItems.length ? (
                  currentItems.map((item, index) => {
                    const isSelected = item.id === selectedItemId;
                    return (
                      <button
                        className={STYLES.historyItem(isSelected)}
                        key={item.id}
                        onClick={() => setSelectedItemId(item.id)}
                        type="button"
                      >
                        <div className="flex items-start justify-between gap-4">
                          <div className="flex-1 min-w-0">
                            <div className={`inline-flex h-6 min-w-6 items-center justify-center rounded-md px-1.5 text-[10px] font-bold transition-colors ${isSelected
                                ? "bg-[color:var(--cp-accent-primary)]/20 text-[color:var(--cp-accent-primary)] ring-1 ring-[color:var(--cp-accent-primary)]/30"
                                : "bg-[color:var(--cp-control-surface)] text-[color:var(--cp-text-muted)] group-hover:bg-[color:var(--cp-control-surface-hover)] group-hover:text-[color:var(--cp-text-secondary)]"
                              }`}>
                              {String(searchQuery.offset + index + 1).padStart(2, "0")}
                            </div>
                            <p
                              className={`mt-3.5 line-clamp-4 text-[13px] font-medium leading-[1.6] whitespace-pre-wrap break-words [overflow-wrap:anywhere] transition-colors ${isSelected ? "text-[color:var(--cp-text-primary)]" : "text-[color:var(--cp-text-secondary)] group-hover:text-[color:var(--cp-text-primary)]"
                                }`}
                              title={item.tooltipText || item.contentPreview}
                            >
                              {item.contentPreview}
                            </p>
                          </div>
                          <div className="flex shrink-0 flex-col items-end gap-2">
                            <span className="inline-flex items-center rounded-full border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[color:var(--cp-control-surface)]/50 px-2.5 py-1 text-[10px] font-bold text-[color:var(--cp-text-muted)] shadow-none">
                              {getClipTypeLabel(item)}
                            </span>
                            {item.isFavorited ? (
                              <span className="inline-flex items-center gap-1 rounded-full bg-[color:var(--cp-favorite)]/15 px-2 py-1 text-[10px] font-bold text-[color:var(--cp-favorite)] ring-1 ring-inset ring-[color:var(--cp-favorite)]/20">
                                <svg className="h-2.5 w-2.5" fill="currentColor" viewBox="0 0 20 20">
                                  <path d="M9.049 2.927c.3-.921 1.603-.921 1.902 0l1.07 3.292a1 1 0 00.95.69h3.462c.969 0 1.371 1.24.588 1.81l-2.8 2.034a1 1 0 00-.364 1.118l1.07 3.292c.3.921-.755 1.688-1.54 1.118l-2.8-2.034a1 1 0 00-1.175 0l-2.8 2.034c-.784.57-1.838-.197-1.539-1.118l1.07-3.292a1 1 0 00-.364-1.118L2.98 8.72c-.783-.57-.38-1.81.588-1.81h3.461a1 1 0 00.951-.69l1.07-3.292z" />
                                </svg>
                                收藏
                              </span>
                            ) : null}
                          </div>
                        </div>
                        <div className="mt-4 flex flex-wrap gap-x-4 gap-y-2 text-[10px] font-bold uppercase tracking-wider text-[color:var(--cp-text-muted)] opacity-80">
                          <span className="flex items-center gap-1.5">
                            <span className={`h-1 w-1 rounded-full ${isSelected ? "bg-[color:var(--cp-accent-primary)]" : "bg-[color:var(--cp-border-strong)]"}`}></span>
                            {item.sourceApp ?? "未知来源"}
                          </span>
                          <span className="flex items-center gap-1.5">
                            <span className="h-1 w-1 rounded-full bg-[color:var(--cp-border-strong)]"></span>
                            创建于 {formatDateTime(item.createdAt)}
                          </span>
                          <span className="flex items-center gap-1.5">
                            <span className="h-1 w-1 rounded-full bg-[color:var(--cp-border-strong)]"></span>
                            最近使用 {formatDateTime(item.lastUsedAt)}
                          </span>
                        </div>
                      </button>
                    );
                  })
                ) : (
                  <EmptyState
                    title="当前没有匹配结果"
                    description="复制文本、图片或文件后会自动入库；如果在浏览器预览模式中运行，这里会展示模拟数据。"
                  />
                )}
              </div>

              <div className="flex shrink-0 items-center justify-between gap-3 border-t border-[rgba(var(--cp-surface1-rgb),0.2)] px-1 pb-1 pt-4 text-[11px] font-bold uppercase tracking-wider text-[color:var(--cp-text-muted)]">
                <span>{totalCount === 0 ? "当前没有可显示记录" : `当前显示第 ${pageStart}-${pageEnd} 条`}</span>
                <div className="flex items-center gap-2">
                  <button
                    className="flex items-center gap-1.5 rounded-md bg-[color:var(--cp-control-surface)]/40 px-3.5 py-2 text-xs font-bold text-[color:var(--cp-text-secondary)] shadow-xs ring-1 ring-inset ring-[rgba(var(--cp-surface1-rgb),0.2)] transition-all hover:bg-[color:var(--cp-control-surface-hover)]/60 hover:text-[color:var(--cp-text-primary)] disabled:cursor-not-allowed disabled:opacity-40"
                    disabled={!hasPreviousPage}
                    onClick={() => setPageIndex((current) => Math.max(current - 1, 0))}
                    type="button"
                  >
                    <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M15 19l-7-7 7-7" />
                    </svg>
                    上一页
                  </button>
                  <button
                    className="flex items-center gap-1.5 rounded-md bg-[color:var(--cp-control-surface)]/40 px-3.5 py-2 text-xs font-bold text-[color:var(--cp-text-secondary)] shadow-xs ring-1 ring-inset ring-[rgba(var(--cp-surface1-rgb),0.2)] transition-all hover:bg-[color:var(--cp-control-surface-hover)]/60 hover:text-[color:var(--cp-text-primary)] disabled:cursor-not-allowed disabled:opacity-40"
                    disabled={!hasNextPage}
                    onClick={() => setPageIndex((current) => current + 1)}
                    type="button"
                  >
                    下一页
                    <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M9 5l7 7-7 7" />
                    </svg>
                  </button>
                </div>
              </div>
            </>
          ) : (
            <div className="h-full overflow-y-auto pr-2">
              <SettingsPanel
                errorMessage={settingsSaveError}
                isPending={updateSettingsMutation.isPending}
                onDismissError={() => updateSettingsMutation.reset()}
                onSave={(nextValue) => {
                  updateSettingsMutation.reset();
                  updateSettingsMutation.mutate(nextValue);
                }}
              />
            </div>
          )}
        </Panel>

        <Panel className="flex flex-col gap-4 lg:overflow-hidden min-h-[500px] lg:min-h-0">
          {detail.isLoading ? (
            <div className="flex h-full items-center justify-center">
              <LoadingSpinner text="加载详情..." />
            </div>
          ) : detail.data && selectedSummary ? (
            <>
              <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b border-[rgba(var(--cp-surface1-rgb),0.2)] pb-4">
                <div>
                  <div className="flex items-center gap-2">
                    <span className="flex h-1.5 w-1.5 rounded-full bg-[color:var(--cp-accent-primary)]"></span>
                    <p className="text-[10px] font-bold uppercase tracking-[0.22em] text-[color:var(--cp-text-muted)]">
                      详情编辑
                    </p>
                  </div>
                  <h2 className="mt-2 font-display text-2xl font-medium tracking-tight text-[color:var(--cp-text-primary)]">
                    {getClipTypeLabel(detail.data)}剪贴项
                  </h2>
                </div>
                <div className="flex gap-2">
                  <button
                     className={`flex items-center gap-1.5 rounded-md px-4 py-2 text-sm font-bold transition-all duration-300 ${detail.data.isFavorited
                        ? "bg-[color:var(--cp-favorite)]/15 text-[color:var(--cp-favorite)] ring-1 ring-inset ring-[color:var(--cp-favorite)]/30 hover:bg-[color:var(--cp-favorite)]/25"
                        : "bg-[color:var(--cp-control-surface)]/40 text-[color:var(--cp-text-secondary)] shadow-none ring-1 ring-inset ring-[rgba(var(--cp-surface1-rgb),0.2)] hover:bg-[color:var(--cp-control-surface-hover)]/60 hover:text-[color:var(--cp-text-primary)]"
                      }`}
                    onClick={() =>
                      favoritedMutation.mutate({
                        id: detail.data.id,
                        value: !detail.data.isFavorited,
                      })
                    }
                    type="button"
                  >
                    <svg className={`h-4 w-4 ${detail.data.isFavorited ? "text-[color:var(--cp-favorite)]" : "text-[color:var(--cp-text-muted)]"}`} fill="currentColor" viewBox="0 0 20 20">
                      <path d="M9.049 2.927c.3-.921 1.603-.921 1.902 0l1.07 3.292a1 1 0 00.95.69h3.462c.969 0 1.371 1.24.588 1.81l-2.8 2.034a1 1 0 00-.364 1.118l1.07 3.292c.3.921-.755 1.688-1.54 1.118l-2.8-2.034a1 1 0 00-1.175 0l-2.8 2.034c-.784.57-1.838-.197-1.539-1.118l1.07-3.292a1 1 0 00-.364-1.118L2.98 8.72c-.783-.57-.38-1.81.588-1.81h3.461a1 1 0 00.951-.69l1.07-3.292z" />
                    </svg>
                    {detail.data.isFavorited ? "已收藏" : "加入收藏"}
                  </button>
                  <button
                    className="group relative rounded-md bg-[color:var(--cp-accent-primary)] px-4 py-2 text-sm font-bold text-cp-base shadow-sm transition-all duration-300 hover:bg-[color:var(--cp-accent-primary-strong)] active:translate-y-0"
                    onClick={() =>
                      pasteMutation.mutate({
                        id: detail.data.id,
                        option: {
                          restoreClipboardAfterPaste:
                          settings.data?.restoreClipboardAfterPaste ?? true,
                          pasteToTarget: false,
                        },
                      })
                    }
                    type="button"
                  >
                    <span className="flex items-center gap-1.5">
                      <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                        <path strokeLinecap="round" strokeLinejoin="round" d="M8 5H6a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2v-1M8 5a2 2 0 002 2h2a2 2 0 002-2M8 5a2 2 0 012-2h2a2 2 0 012 2m0 0h2a2 2 0 012 2v3m2 4H10m0 0l3-3m-3 3l3 3" />
                      </svg>
                      {detail.data.type === "text" ? "写入剪贴板" : "复制到剪贴板"}
                    </span>
                  </button>
                </div>
              </div>

              <dl className="grid shrink-0 gap-3 sm:grid-cols-2 text-sm">
                <div className="flex items-center justify-between sm:block py-2 border-b border-[rgba(var(--cp-surface1-rgb),0.1)]">
                  <dt className="text-[color:var(--cp-text-muted)]">来源应用</dt>
                  <dd className="font-medium text-[color:var(--cp-text-primary)] sm:mt-1">{selectedSummary.sourceApp ?? "未知来源"}</dd>
                </div>
                <div className="flex items-center justify-between sm:block py-2 border-b border-[rgba(var(--cp-surface1-rgb),0.1)]">
                  <dt className="text-[color:var(--cp-text-muted)]">最近使用</dt>
                  <dd className="font-medium text-[color:var(--cp-text-primary)] sm:mt-1">{formatDateTime(selectedSummary.lastUsedAt)}</dd>
                </div>
                <div className="flex items-center justify-between sm:block py-2 border-b border-[rgba(var(--cp-surface1-rgb),0.1)]">
                  <dt className="text-[color:var(--cp-text-muted)]">创建时间</dt>
                  <dd className="font-medium text-[color:var(--cp-text-primary)] sm:mt-1">{formatDateTime(selectedSummary.createdAt)}</dd>
                </div>
                <div className="flex items-center justify-between sm:block py-2 border-b border-[rgba(var(--cp-surface1-rgb),0.1)]">
                  <dt className="text-[color:var(--cp-text-muted)]">更新时间</dt>
                  <dd className="font-medium text-[color:var(--cp-text-primary)] sm:mt-1">{formatDateTime(selectedSummary.updatedAt)}</dd>
                </div>
                {/* 图片或文件类型的额外信息 */}
                {detail.data.type === "image" && (
                  <>
                    <div className="flex items-center justify-between sm:block py-2 border-b border-[rgba(var(--cp-surface1-rgb),0.1)]">
                      <dt className="text-[color:var(--cp-text-muted)]">尺寸</dt>
                      <dd className="font-medium text-[color:var(--cp-text-primary)] sm:mt-1">{detail.data.imageWidth} × {detail.data.imageHeight}</dd>
                    </div>
                    <div className="flex items-center justify-between sm:block py-2 border-b border-[rgba(var(--cp-surface1-rgb),0.1)]">
                      <dt className="text-[color:var(--cp-text-muted)]">大小</dt>
                      <dd className="font-medium text-[color:var(--cp-text-primary)] sm:mt-1">{formatBytes(detail.data.fileSize)}</dd>
                    </div>
                    {detail.data.imageFormat && (
                      <div className="flex items-center justify-between sm:block py-2 border-b border-[rgba(var(--cp-surface1-rgb),0.1)]">
                        <dt className="text-[color:var(--cp-text-muted)]">格式</dt>
                        <dd className="font-medium text-[color:var(--cp-text-primary)] sm:mt-1">{detail.data.imageFormat}</dd>
                      </div>
                    )}
                  </>
                )}
                {detail.data.type === "file" && (
                  <>
                    <div className="flex items-center justify-between sm:block py-2 border-b border-[rgba(var(--cp-surface1-rgb),0.1)]">
                      <dt className="text-[color:var(--cp-text-muted)]">{getFileCountLabel(detail.data.fileCount, detail.data.directoryCount)}</dt>
                      <dd className="font-medium text-[color:var(--cp-text-primary)] sm:mt-1">{detail.data.fileCount} 个</dd>
                    </div>
                    <div className="flex items-center justify-between sm:block py-2 border-b border-[rgba(var(--cp-surface1-rgb),0.1)]">
                      <dt className="text-[color:var(--cp-text-muted)]">总大小</dt>
                      <dd className="font-medium text-[color:var(--cp-text-primary)] sm:mt-1">
                        {detail.data.directoryCount > 0 ? "未统计" : formatBytes(detail.data.totalSize)}
                        {detail.data.directoryCount > 0 && (
                          <span className="ml-2 text-[10px] text-[color:var(--cp-text-muted)]">包含文件夹时默认不递归统计大小。</span>
                        )}
                      </dd>
                    </div>
                  </>
                )}
              </dl>

              {/* 根据类型显示内容 */}
              {detail.data.type === "text" ? (
                <>
                  <textarea
                    className={STYLES.detailEditor}
                    onChange={(event) => setDraftText(event.target.value)}
                    value={draftText}
                  />

                   <div className="flex shrink-0 flex-wrap gap-2 pt-2">
                     <button
                       className="rounded-md border border-[color:var(--cp-border-weak)] bg-[rgba(var(--cp-surface0-rgb),0.2)] px-5 py-2.5 text-sm font-bold text-[color:var(--cp-text-primary)] shadow-none transition-all duration-300 hover:border-[color:var(--cp-border-weak)] hover:bg-[rgba(var(--cp-surface1-rgb),0.2)] active:scale-95 disabled:opacity-50"
                       disabled={updateTextMutation.isPending}
                       onClick={() =>
                         updateTextMutation.mutate({
                           id: detail.data.id,
                           text: draftText,
                         })
                       }
                       type="button"
                     >
                       <span className="flex items-center gap-2">
                         <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                           <path strokeLinecap="round" strokeLinejoin="round" d="M8 7H5a2 2 0 00-2 2v9a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-3m-1 4l-3 3m0 0l-3-3m3 3V4" />
                         </svg>
                         保存文本修改
                       </span>
                     </button>
                     <button
                       className="rounded-md border border-[rgba(var(--cp-red-rgb),0.2)] bg-[rgba(var(--cp-red-rgb),0.05)] px-5 py-2.5 text-sm font-bold text-[color:var(--cp-danger)] transition-all duration-300 hover:bg-[rgba(var(--cp-red-rgb),0.15)] hover:border-[rgba(var(--cp-red-rgb),0.35)] active:scale-95 disabled:opacity-50"
                       disabled={deleteMutation.isPending}
                       onClick={() => {
                         deleteMutation.mutate(detail.data.id, {
                           onSuccess: () => setSelectedItemId(null),
                         });
                       }}
                       type="button"
                     >
                       删除记录
                     </button>
                   </div>
                </>
              ) : (
                <>
                  <div className="flex flex-col items-center justify-center py-10 text-center">
                    <div className="text-6xl mb-3">{getClipTypeIcon(detail.data)}</div>
                    <p className="mb-1 text-sm font-medium text-[color:var(--cp-text-secondary)]">
                      {detail.data.type === "image"
                        ? "图片类型记录不支持文本编辑，你可以复制到剪贴板后手动粘贴"
                        : detail.data.type === "file"
                          ? `${getClipTypeLabel(detail.data)}类型记录不支持文本编辑，你可以复制到剪贴板后手动粘贴`
                          : "该类型记录不支持文本编辑"}
                    </p>
                    {detail.data.type === "file" && (
                      <div className="mt-4 w-full rounded-md border border-[rgba(var(--cp-surface2-rgb),0.35)] bg-cp-mantle p-4 text-left dark:border-[rgba(var(--cp-surface1-rgb),0.2)] dark:bg-[rgba(var(--cp-surface0-rgb),0.2)]">
                        <p className="mb-2 text-[10px] font-bold uppercase tracking-wide text-[color:var(--cp-text-muted)]">
                          文件路径
                        </p>
                        <div className="space-y-1 text-xs font-medium text-[color:var(--cp-text-primary)]">
                          {detail.data.filePaths.map((path, index) => (
                            <p key={index} className="truncate hover:whitespace-normal">
                              {path}
                            </p>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                   <div className="flex shrink-0 flex-wrap gap-2 pt-2">
                     <button
                       className="rounded-md border border-[rgba(var(--cp-red-rgb),0.2)] bg-[rgba(var(--cp-red-rgb),0.05)] px-5 py-2.5 text-sm font-bold text-[color:var(--cp-danger)] transition-all duration-300 hover:bg-[rgba(var(--cp-red-rgb),0.15)] hover:border-[rgba(var(--cp-red-rgb),0.35)] active:scale-95 disabled:opacity-50"
                       disabled={deleteMutation.isPending}
                       onClick={() => {
                         deleteMutation.mutate(detail.data.id, {
                           onSuccess: () => setSelectedItemId(null),
                         });
                       }}
                       type="button"
                     >
                       删除记录
                     </button>
                   </div>
                </>
              )}

              {pasteMutation.data ? (
                <p className="shrink-0 rounded-md bg-[color:var(--cp-favorite)]/15 px-4 py-3 text-sm font-bold text-[color:var(--cp-favorite)] ring-1 ring-inset ring-[color:var(--cp-favorite)]/20 animate-in fade-in slide-in-from-bottom-2">
                  {pasteMutation.data.message}
                </p>
              ) : null}
            </>
          ) : (
            <EmptyState
              title="选择一条记录开始编辑"
              description="为避免启动时直接加载超长全文，资料库不再自动选中首条记录，请从中间列表手动选择。"
            />
          )}
        </Panel>
      </div>
    </main>
  );
}

interface SettingsPanelProps {
  errorMessage: string | null;
  isPending: boolean;
  onDismissError: () => void;
  onSave: (payload: UserSetting) => void;
}

function SettingsPanel({ errorMessage, isPending, onDismissError, onSave }: SettingsPanelProps) {
  const { data, isLoading } = useSettingsQuery();
  const [shortcut, setShortcut] = useState("Ctrl+`");
  const [launchOnStartup, setLaunchOnStartup] = useState(false);
  const [silentOnStartup, setSilentOnStartup] = useState(false);
  const [historyLimit, setHistoryLimit] = useState(1000);
  const [pickerRecordLimit, setPickerRecordLimit] = useState(50);
  const [pickerPositionMode, setPickerPositionMode] = useState<PickerPositionMode>("mouse");
  const [restoreClipboardAfterPaste, setRestoreClipboardAfterPaste] = useState(true);
  const [pauseMonitoring, setPauseMonitoring] = useState(false);
  const [themeMode, setThemeMode] = useState<ThemeMode>("system");
  const [excludedAppsText, setExcludedAppsText] = useState("");
  const [workbenchShortcut, setWorkbenchShortcut] = useState("Win+F");
  const [workbenchShortcutEnabled, setWorkbenchShortcutEnabled] = useState(true);

  useEffect(() => {
    if (!data) {
      return;
    }

    setShortcut(data.shortcut);
    setLaunchOnStartup(data.launchOnStartup);
    setSilentOnStartup(data.silentOnStartup);
    setHistoryLimit(data.historyLimit);
    setPickerRecordLimit(data.pickerRecordLimit);
    setPickerPositionMode(data.pickerPositionMode);
    setRestoreClipboardAfterPaste(data.restoreClipboardAfterPaste);
    setPauseMonitoring(data.pauseMonitoring);
    setThemeMode(data.themeMode);
    setExcludedAppsText(data.excludedApps.join("\n"));
    setWorkbenchShortcut(data.workbenchShortcut);
    setWorkbenchShortcutEnabled(data.workbenchShortcutEnabled);
  }, [data]);

  if (isLoading && !data) {
    return <EmptyState title="正在加载设置" description="稍后即可编辑快捷键、历史上限和排除应用。" />;
  }

  return (
    <div className="space-y-6">
      <div>
        <p className="text-[11px] font-bold uppercase tracking-[0.22em] text-[color:var(--cp-text-muted)]">设置</p>
        <h2 className="mt-1 font-display text-2xl font-medium tracking-tight text-[color:var(--cp-text-primary)]">运行设置</h2>
        <p className="mt-3 text-[13px] leading-relaxed text-[color:var(--cp-text-secondary)]">
          快捷键、历史记录上限、界面主题等偏好设置会自动保存。
        </p>
      </div>

      {errorMessage ? (
        <div className="flex items-start justify-between gap-3 rounded-md bg-[color:var(--cp-danger)]/15 px-4 py-3 text-sm font-bold text-[color:var(--cp-danger)] ring-1 ring-inset ring-[color:var(--cp-danger)]/20">
          <p className="leading-relaxed">{errorMessage}</p>
          <button
            className="shrink-0 text-xs font-bold uppercase tracking-wider transition-opacity hover:opacity-80"
            onClick={onDismissError}
            type="button"
          >
            关闭
          </button>
        </div>
      ) : null}

      <label className="block">
        <span className="mb-2.5 block text-[13px] font-bold text-[color:var(--cp-text-primary)]">全局快捷键</span>
        <input
          className="w-full rounded-md border border-[rgba(var(--cp-surface1-rgb),0.35)] bg-cp-mantle px-5 py-3.5 text-sm font-medium outline-none transition-all focus:border-[rgba(var(--cp-accent-primary-rgb),0.25)] focus:bg-[color:var(--cp-window-shell)] focus-visible:outline-none dark:border-[rgba(var(--cp-surface1-rgb),0.4)] dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
          onChange={(event) => setShortcut(event.target.value)}
          value={shortcut}
        />
      </label>

      <div className="block">
        <div className="mb-2.5 flex items-center justify-between">
          <span className="text-[13px] font-bold text-[color:var(--cp-text-primary)]">搜索窗口快捷键</span>
          <label className="flex cursor-pointer items-center gap-2">
            <input
              checked={workbenchShortcutEnabled}
              className="h-4 w-4 rounded border-[rgba(var(--cp-surface1-rgb),0.4)] bg-cp-base text-[color:var(--cp-accent-primary)] focus:ring-[color:var(--cp-accent-primary)] dark:bg-[color:var(--cp-control-surface)]"
              onChange={(event) => setWorkbenchShortcutEnabled(event.target.checked)}
              type="checkbox"
            />
            <span className="text-[12px] font-medium text-[color:var(--cp-text-muted)]">启用</span>
          </label>
        </div>
        <input
          className="w-full rounded-md border border-[rgba(var(--cp-surface1-rgb),0.35)] bg-cp-mantle px-5 py-3.5 text-sm font-medium outline-none transition-all focus:border-[rgba(var(--cp-accent-primary-rgb),0.25)] focus:bg-[color:var(--cp-window-shell)] focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50 dark:border-[rgba(var(--cp-surface1-rgb),0.4)] dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
          disabled={!workbenchShortcutEnabled}
          onChange={(event) => setWorkbenchShortcut(event.target.value)}
          placeholder="Win+F"
          value={workbenchShortcut}
        />
        <p className="mt-2 text-[11px] font-medium leading-relaxed text-[color:var(--cp-text-muted)]">
          全局快捷键，直接打开搜索窗口。
        </p>
      </div>

      <label className="block">
        <span className="mb-2.5 block text-[13px] font-bold text-[color:var(--cp-text-primary)]">历史记录上限</span>
        <input
          className="w-full rounded-md border border-[rgba(var(--cp-surface1-rgb),0.35)] bg-cp-mantle px-5 py-3.5 text-sm font-medium outline-none transition-all focus:border-[rgba(var(--cp-accent-primary-rgb),0.25)] focus:bg-[color:var(--cp-window-shell)] focus-visible:outline-none dark:border-[rgba(var(--cp-surface1-rgb),0.4)] dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
          min={100}
          onChange={(event) => setHistoryLimit(Number(event.target.value) || 1000)}
          step={100}
          type="number"
          value={historyLimit}
        />
      </label>

      <label className="block">
        <span className="mb-2.5 block text-[13px] font-bold text-[color:var(--cp-text-primary)]">速贴窗口记录数</span>
        <input
          className="w-full rounded-md border border-[rgba(var(--cp-surface1-rgb),0.35)] bg-cp-mantle px-5 py-3.5 text-sm font-medium outline-none transition-all focus:border-[rgba(var(--cp-accent-primary-rgb),0.25)] focus:bg-[color:var(--cp-window-shell)] focus-visible:outline-none dark:border-[rgba(var(--cp-surface1-rgb),0.4)] dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
          max={1000}
          min={9}
          onChange={(event) => setPickerRecordLimit(Number(event.target.value) || 50)}
          type="number"
          value={pickerRecordLimit}
        />
        <p className="mt-2 text-[11px] font-medium leading-relaxed text-[color:var(--cp-text-muted)]">
          控制速贴面板一次可滚动浏览的记录数，数字快捷键仍只覆盖前 9 条。
        </p>
      </label>

      <fieldset className="block border-0 p-0 m-0">
        <legend className="mb-2.5 block text-[13px] font-bold text-[color:var(--cp-text-primary)]">界面主题</legend>
        <div className="space-y-3">
          {themeModeOptions.map((option) => (
            <label
              className={`flex cursor-pointer items-start gap-3 rounded-md border px-5 py-4 transition-all duration-300 ${themeMode === option.value
                  ? "border-[rgba(var(--cp-accent-primary-rgb),0.2)] bg-[rgba(var(--cp-accent-primary-rgb),0.05)] ring-1 ring-[rgba(var(--cp-accent-primary-rgb),0.1)]"
                  : "border-[rgba(var(--cp-surface1-rgb),0.2)] bg-cp-mantle hover:border-[rgba(var(--cp-surface1-rgb),0.4)] hover:bg-[rgba(var(--cp-surface1-rgb),0.15)]"
                }`}
              key={option.value}
            >
              <input
                checked={themeMode === option.value}
                className="mt-1 h-4 w-4 border-[rgba(var(--cp-surface1-rgb),0.4)] bg-cp-base text-[color:var(--cp-accent-primary)] focus:ring-[color:var(--cp-accent-primary)]"
                name="theme-mode"
                onChange={() => setThemeMode(option.value)}
                type="radio"
              />
              <span className="min-w-0">
                <span className={`block text-sm font-bold transition-colors ${themeMode === option.value ? "text-[color:var(--cp-text-primary)]" : "text-[color:var(--cp-text-secondary)]"}`}>{option.label}</span>
                <span className="mt-1 block text-[11px] font-medium leading-relaxed text-[color:var(--cp-text-muted)] opacity-80">
                  {option.description}
                </span>
              </span>
            </label>
          ))}
        </div>
      </fieldset>

      <fieldset className="border-0 p-0 m-0">
        <legend className="mb-2.5 block text-[13px] font-bold text-[color:var(--cp-text-primary)]">速贴窗口显示位置</legend>
        <div className="space-y-3">
          {pickerPositionOptions.map((option) => (
            <label
              className={`flex cursor-pointer items-start gap-3 rounded-md border px-5 py-4 transition-all duration-300 ${pickerPositionMode === option.value
                  ? "border-[rgba(var(--cp-accent-primary-rgb),0.2)] bg-[rgba(var(--cp-accent-primary-rgb),0.05)] ring-1 ring-[rgba(var(--cp-accent-primary-rgb),0.1)]"
                  : "border-[rgba(var(--cp-surface1-rgb),0.2)] bg-cp-mantle hover:border-[rgba(var(--cp-surface1-rgb),0.4)] hover:bg-[rgba(var(--cp-surface1-rgb),0.15)]"
                }`}
              key={option.value}
            >
              <input
                checked={pickerPositionMode === option.value}
                className="mt-1 h-4 w-4 border-[rgba(var(--cp-surface1-rgb),0.4)] bg-cp-base text-[color:var(--cp-accent-primary)] focus:ring-[color:var(--cp-accent-primary)]"
                name="picker-position-mode"
                onChange={() => setPickerPositionMode(option.value)}
                type="radio"
              />
              <span className="min-w-0">
                <span className={`block text-sm font-bold transition-colors ${pickerPositionMode === option.value ? "text-[color:var(--cp-text-primary)]" : "text-[color:var(--cp-text-secondary)]"}`}>{option.label}</span>
                <span className="mt-1 block text-[11px] font-medium leading-relaxed text-[color:var(--cp-text-muted)] opacity-80">
                  {option.description}
                </span>
              </span>
            </label>
          ))}
        </div>
      </fieldset>

      <label className="block">
        <span className="mb-2.5 block text-[13px] font-bold text-[color:var(--cp-text-primary)]">排除应用</span>
        <textarea
          className="w-full rounded-md border border-[rgba(var(--cp-surface1-rgb),0.35)] bg-cp-mantle px-5 py-3.5 text-sm font-medium leading-relaxed outline-none transition-all focus:border-[rgba(var(--cp-accent-primary-rgb),0.25)] focus:bg-[color:var(--cp-window-shell)] focus-visible:outline-none dark:border-[rgba(var(--cp-surface1-rgb),0.4)] dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
          onChange={(event) => setExcludedAppsText(event.target.value)}
          placeholder={"每行一个可执行文件名，例如：\nKeePass.exe\nWindowsTerminal.exe"}
          value={excludedAppsText}
        />
      </label>

      <div className="space-y-3">
        <label className="flex cursor-pointer items-center gap-3 rounded-md border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-cp-mantle px-5 py-4 transition-all duration-300 hover:border-[rgba(var(--cp-surface1-rgb),0.4)]/30 hover:bg-[rgba(var(--cp-surface1-rgb),0.2)]">
          <input
            className="h-4 w-4 rounded border-[rgba(var(--cp-surface1-rgb),0.4)] bg-cp-base text-[color:var(--cp-accent-primary)] focus:ring-[color:var(--cp-accent-primary)] dark:bg-[color:var(--cp-control-surface)]"
            checked={launchOnStartup}
            onChange={(event) => {
              const checked = event.target.checked;
              setLaunchOnStartup(checked);
              if (!checked) {
                setSilentOnStartup(false);
              }
            }}
            type="checkbox"
          />
          <span className="text-sm font-bold text-[color:var(--cp-text-secondary)] transition-colors">开机自启</span>
        </label>

        <label
          className={`flex items-center gap-3 rounded-md border px-5 py-4 transition-all duration-300 ${launchOnStartup
              ? "cursor-pointer border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[rgba(var(--cp-surface0-rgb),0.2)] hover:border-[rgba(var(--cp-surface1-rgb),0.4)]/30 hover:bg-[rgba(var(--cp-surface1-rgb),0.2)]"
              : "cursor-not-allowed border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[rgba(var(--cp-surface0-rgb),0.1)] text-[color:var(--cp-text-muted)]"
            }`}
        >
          <input
            className="h-4 w-4 rounded border-[rgba(var(--cp-surface1-rgb),0.4)] bg-cp-base text-[color:var(--cp-accent-primary)] focus:ring-[color:var(--cp-accent-primary)] dark:bg-[color:var(--cp-control-surface)]"
            checked={silentOnStartup}
            disabled={!launchOnStartup}
            onChange={(event) => setSilentOnStartup(event.target.checked)}
            type="checkbox"
          />
          <span className={`text-sm font-bold transition-colors ${launchOnStartup ? "text-[color:var(--cp-text-secondary)]" : "text-[color:var(--cp-text-muted)]"}`}>开机自启时静默启动</span>
        </label>

        <label className="flex cursor-pointer items-center gap-3 rounded-md border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-cp-mantle px-5 py-4 transition-all duration-300 hover:border-[rgba(var(--cp-surface1-rgb),0.4)]/30 hover:bg-[rgba(var(--cp-surface1-rgb),0.2)]">
          <input
            className="h-4 w-4 rounded border-[rgba(var(--cp-surface1-rgb),0.4)] bg-cp-base text-[color:var(--cp-accent-primary)] focus:ring-[color:var(--cp-accent-primary)] dark:bg-[color:var(--cp-control-surface)]"
            checked={restoreClipboardAfterPaste}
            onChange={(event) => setRestoreClipboardAfterPaste(event.target.checked)}
            type="checkbox"
          />
          <span className="text-sm font-bold text-[color:var(--cp-text-secondary)]">回贴后恢复原始剪贴板</span>
        </label>

        <label className="flex cursor-pointer items-center gap-3 rounded-md border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-cp-mantle px-5 py-4 transition-all duration-300 hover:border-[rgba(var(--cp-surface1-rgb),0.4)]/30 hover:bg-[rgba(var(--cp-surface1-rgb),0.2)]">
          <input
            className="h-4 w-4 rounded border-[rgba(var(--cp-surface1-rgb),0.4)] bg-cp-base text-[color:var(--cp-accent-primary)] focus:ring-[color:var(--cp-accent-primary)] dark:bg-[color:var(--cp-control-surface)]"
            checked={pauseMonitoring}
            onChange={(event) => setPauseMonitoring(event.target.checked)}
            type="checkbox"
          />
          <span className="text-sm font-bold text-[color:var(--cp-text-secondary)]">暂停监听</span>
        </label>
      </div>

      <div className="pt-2">
        <button
          className="rounded-md bg-[color:var(--cp-accent-primary)] px-8 py-4 text-sm font-bold text-cp-base shadow-sm transition-all duration-300 hover:bg-[color:var(--cp-accent-primary-strong)] active:scale-95 disabled:opacity-50"
          disabled={isPending}
          onClick={() =>
            onSave({
              shortcut,
              launchOnStartup,
              silentOnStartup: launchOnStartup ? silentOnStartup : false,
              historyLimit,
              pickerRecordLimit,
              pickerPositionMode,
              themeMode,
              excludedApps: excludedAppsText
                .split(/\r?\n/)
                .map((value) => value.trim())
                .filter(Boolean),
              restoreClipboardAfterPaste,
              pauseMonitoring,
              workbenchShortcut,
              workbenchShortcutEnabled,
            })
          }
          type="button"
        >
          保存设置
        </button>
      </div>
    </div>
  );
}

function getErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }

  if (typeof error === "string" && error.trim()) {
    return error;
  }

  return "保存设置失败，请稍后重试。";
}

