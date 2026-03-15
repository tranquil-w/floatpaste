import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Panel } from "../../shared/ui/Panel";
import { StatusBadge } from "../../shared/ui/StatusBadge";
import { EmptyState } from "../../shared/components/EmptyState";
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
    <div className="flex min-h-screen flex-col overflow-y-auto px-4 py-6 text-ink lg:h-screen lg:overflow-hidden md:px-6">
      <div className="mx-auto grid w-full max-w-[1600px] flex-1 min-h-0 gap-4 xl:grid-cols-[280px_minmax(360px,1fr)_420px] lg:grid-cols-[260px_minmax(320px,1fr)_380px]">
        <Panel className="flex flex-col gap-5 lg:overflow-hidden min-h-[600px] lg:min-h-0">
          <div className="shrink-0">
            <div className="inline-flex items-center gap-2 rounded-full border border-[color:var(--cp-accent-primary)]/20 bg-[color:var(--cp-accent-primary)]/10 pl-1.5 pr-3 py-1 text-[10px] font-bold uppercase tracking-[0.2em] text-[color:var(--cp-accent-primary)] shadow-sm">
              <span className="flex h-5 w-5 items-center justify-center rounded-full bg-[color:var(--cp-accent-primary)] text-cp-base shadow-sm">
                <svg className="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
                </svg>
              </span>
              FloatPaste
            </div>
            <h1 className="mt-4 font-display text-[2rem] font-medium tracking-tight text-[color:var(--cp-text-primary)]">资料库窗口</h1>
            <p className="mt-3 text-[13px] leading-relaxed text-[color:var(--cp-text-secondary)]">
              当前版本已经打通文本、图片、文件记录的入库与基础浏览，搜索、收藏、设置与速贴主链路也已接入。
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

          <div className="relative shrink-0 overflow-hidden rounded-3xl bg-[color:var(--cp-card-surface)]/40 px-6 py-6 text-[color:var(--cp-text-primary)] shadow-xl shadow-black/5 ring-1 ring-[rgba(var(--cp-surface1-rgb),0.2)] transition-transform duration-500 hover:scale-[1.01] dark:bg-[color:var(--cp-card-surface)]/30 dark:shadow-none dark:ring-[rgba(var(--cp-surface1-rgb),0.2)]/50">
            <div className="relative z-10">
              <div className="flex items-center gap-2">
                <div className="h-1.5 w-1.5 rounded-full bg-[color:var(--cp-accent-primary)] shadow-[0_0_8px_rgba(114,135,253,0.5)]"></div>
                <p className="text-[10px] font-bold uppercase tracking-[0.25em] text-[color:var(--cp-text-muted)]">全局快捷键</p>
              </div>
              <p className="mt-2.5 font-display text-3xl font-medium tracking-wide text-[color:var(--cp-text-primary)]">{settings.data?.shortcut ?? "Ctrl+`"}</p>
              <p className="mt-3 text-xs leading-relaxed text-[color:var(--cp-text-muted)] font-medium">唤起速贴面板进行剪贴板管理</p>
            </div>
            <div className="absolute -right-10 -top-10 h-40 w-40 rounded-full bg-[color:var(--cp-accent-primary)]/10 blur-[40px] transition-all duration-700 group-hover:bg-[color:var(--cp-accent-primary)]/20"></div>
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
                    className="group w-full rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.3)] bg-[rgba(var(--cp-surface0-rgb),0.2)] px-4 py-3 text-left transition-all duration-300 hover:-translate-y-0.5 hover:border-[rgba(var(--cp-surface1-rgb),0.5)] hover:bg-[rgba(var(--cp-surface1-rgb),0.2)] dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
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
                className="group relative flex w-full items-center justify-center gap-2 rounded-2xl bg-[color:var(--cp-accent-primary)] px-4 py-3.5 text-sm font-bold text-cp-base shadow-lg shadow-[color:var(--cp-accent-primary)]/20 transition-all duration-300 hover:-translate-y-0.5 hover:shadow-xl hover:shadow-[color:var(--cp-accent-primary)]/30 hover:brightness-110 active:translate-y-0"
                onClick={() => void showPicker()}
                type="button"
              >
                <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M14 5l7 7m0 0l-7 7m7-7H3" />
                </svg> open picker
              </button>
              <div className="grid grid-cols-2 gap-2 rounded-2xl bg-[color:var(--cp-control-surface)]/40 p-1.5 backdrop-blur-sm ring-1 ring-[rgba(var(--cp-surface1-rgb),0.2)]">
                <button
                  className={`flex items-center justify-center gap-2 rounded-xl px-4 py-2.5 text-sm font-bold transition-all duration-300 ${viewMode === "history"
                      ? "bg-[color:var(--cp-window-shell)] text-[color:var(--cp-text-primary)] shadow-sm ring-1 ring-[rgba(var(--cp-surface1-rgb),0.2)]"
                      : "text-[color:var(--cp-text-secondary)] hover:text-[color:var(--cp-text-primary)] hover:bg-[color:var(--cp-control-surface-hover)]/40"
                    }`}
                  onClick={() => setViewMode("history")}
                  type="button"
                >
                  <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2.5} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                  </svg>
                  历史库
                </button>
                <button
                  className={`flex items-center justify-center gap-2 rounded-xl px-4 py-2.5 text-sm font-bold transition-all duration-300 ${viewMode === "settings"
                      ? "bg-[color:var(--cp-window-shell)] text-[color:var(--cp-text-primary)] shadow-sm ring-1 ring-[rgba(var(--cp-surface1-rgb),0.2)]"
                      : "text-[color:var(--cp-text-secondary)] hover:text-[color:var(--cp-text-primary)] hover:bg-[color:var(--cp-control-surface-hover)]/40"
                    }`}
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
                  <div className="absolute inset-y-0 left-0 flex items-center pl-4 text-[color:var(--cp-text-muted)] transition-colors pointer-events-none group-focus-within:text-[color:var(--cp-accent-primary)]">
                    <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                    </svg>
                  </div>
                  <input
                    className="w-full rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.4)] bg-[rgba(var(--cp-surface0-rgb),0.2)] py-3 pl-11 pr-5 text-sm outline-none backdrop-blur-sm transition-all duration-300 placeholder:text-[color:var(--cp-text-muted)] focus:border-[rgba(var(--cp-lavender-rgb),0.4)] focus:bg-[color:var(--cp-window-shell)] focus:ring-4 focus:ring-[rgba(var(--cp-lavender-rgb),0.1)] dark:bg-[rgba(var(--cp-surface0-rgb),0.3)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
                    onChange={(event) => setKeyword(event.target.value)}
                    placeholder="搜索全文、来源应用或关键短语"
                    value={keyword}
                  />
                </div>
                <label className="inline-flex cursor-pointer items-center gap-2 rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.4)] bg-[rgba(var(--cp-surface0-rgb),0.2)] px-5 py-3 text-sm font-semibold text-[color:var(--cp-text-secondary)] shadow-sm transition-all duration-300 hover:border-[rgba(var(--cp-surface1-rgb),0.6)] hover:bg-[rgba(var(--cp-surface1-rgb),0.2)] hover:text-[color:var(--cp-text-primary)] active:scale-95">
                  <input
                    className="h-4 w-4 rounded border-[rgba(var(--cp-surface1-rgb),0.6)] bg-cp-base text-[color:var(--cp-accent-primary)] focus:ring-[color:var(--cp-accent-primary)]"
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
                {currentItems.length ? (
                  currentItems.map((item, index) => {
                    const isSelected = item.id === selectedItemId;
                    return (
                      <button
                        className={`group w-full rounded-3xl border px-6 py-5 text-left transition-all duration-300 ${isSelected
                            ? "relative z-10 scale-[1.01] border-[rgba(var(--cp-lavender-rgb),0.4)] bg-[color:var(--cp-window-shell)] shadow-xl shadow-black/5 ring-1 ring-[rgba(var(--cp-lavender-rgb),0.3)] dark:bg-[rgba(var(--cp-surface0-rgb),0.6)] dark:shadow-none dark:ring-[rgba(var(--cp-lavender-rgb),0.4)]"
                            : "border-[rgba(var(--cp-surface1-rgb),0.3)] bg-[rgba(var(--cp-card-surface-rgb),0.3)] hover:-translate-y-0.5 hover:border-[rgba(var(--cp-surface1-rgb),0.6)] hover:bg-[rgba(var(--cp-card-surface-rgb),0.6)] hover:shadow-md dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
                          }`}
                        key={item.id}
                        onClick={() => setSelectedItemId(item.id)}
                        type="button"
                      >
                        <div className="flex items-start justify-between gap-4">
                          <div className="flex-1 min-w-0">
                            <div className={`inline-flex h-6 min-w-6 items-center justify-center rounded-lg px-1.5 text-[10px] font-bold transition-colors ${isSelected
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
                            <span className="inline-flex items-center rounded-full border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[color:var(--cp-control-surface)]/50 px-2.5 py-1 text-[10px] font-bold text-[color:var(--cp-text-muted)] shadow-sm">
                              {getClipTypeLabel(item)}
                            </span>
                            {item.isFavorited ? (
                              <span className="inline-flex items-center gap-1 rounded-full bg-[color:var(--cp-accent-warm)]/15 px-2 py-1 text-[10px] font-bold text-[color:var(--cp-accent-warm)] ring-1 ring-inset ring-[color:var(--cp-accent-warm)]/20">
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
                    className="flex items-center gap-1.5 rounded-xl bg-[color:var(--cp-control-surface)]/40 px-3.5 py-2 text-xs font-bold text-[color:var(--cp-text-secondary)] shadow-sm ring-1 ring-inset ring-[rgba(var(--cp-surface1-rgb),0.2)] transition-all hover:bg-[color:var(--cp-control-surface-hover)]/60 hover:text-[color:var(--cp-text-primary)] disabled:cursor-not-allowed disabled:opacity-40"
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
                    className="flex items-center gap-1.5 rounded-xl bg-[color:var(--cp-control-surface)]/40 px-3.5 py-2 text-xs font-bold text-[color:var(--cp-text-secondary)] shadow-sm ring-1 ring-inset ring-[rgba(var(--cp-surface1-rgb),0.2)] transition-all hover:bg-[color:var(--cp-control-surface-hover)]/60 hover:text-[color:var(--cp-text-primary)] disabled:cursor-not-allowed disabled:opacity-40"
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
          {detail.data && selectedSummary ? (
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
                    className={`flex items-center gap-1.5 rounded-xl px-4 py-2 text-sm font-bold transition-all duration-300 ${detail.data.isFavorited
                        ? "bg-[color:var(--cp-accent-warm)]/15 text-[color:var(--cp-accent-warm)] ring-1 ring-inset ring-[color:var(--cp-accent-warm)]/30 hover:bg-[color:var(--cp-accent-warm)]/25"
                        : "bg-[color:var(--cp-control-surface)]/40 text-[color:var(--cp-text-secondary)] shadow-sm ring-1 ring-inset ring-[rgba(var(--cp-surface1-rgb),0.2)] hover:bg-[color:var(--cp-control-surface-hover)]/60 hover:text-[color:var(--cp-text-primary)]"
                      }`}
                    onClick={() =>
                      favoritedMutation.mutate({
                        id: detail.data.id,
                        value: !detail.data.isFavorited,
                      })
                    }
                    type="button"
                  >
                    <svg className={`h-4 w-4 ${detail.data.isFavorited ? "text-[color:var(--cp-accent-warm)]" : "text-[color:var(--cp-text-muted)]"}`} fill="currentColor" viewBox="0 0 20 20">
                      <path d="M9.049 2.927c.3-.921 1.603-.921 1.902 0l1.07 3.292a1 1 0 00.95.69h3.462c.969 0 1.371 1.24.588 1.81l-2.8 2.034a1 1 0 00-.364 1.118l1.07 3.292c.3.921-.755 1.688-1.54 1.118l-2.8-2.034a1 1 0 00-1.175 0l-2.8 2.034c-.784.57-1.838-.197-1.539-1.118l1.07-3.292a1 1 0 00-.364-1.118L2.98 8.72c-.783-.57-.38-1.81.588-1.81h3.461a1 1 0 00.951-.69l1.07-3.292z" />
                    </svg>
                    {detail.data.isFavorited ? "已收藏" : "加入收藏"}
                  </button>
                  <button
                    className="group relative rounded-xl bg-[color:var(--cp-accent-primary)] px-4 py-2 text-sm font-bold text-cp-base shadow-md shadow-[color:var(--cp-accent-primary)]/10 transition-all duration-300 hover:-translate-y-0.5 hover:shadow-lg hover:brightness-110 active:translate-y-0"
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

              <div className="grid shrink-0 gap-3 sm:grid-cols-2">
                <div className="rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[color:var(--cp-control-surface)]/30 p-4 transition-all hover:bg-[color:var(--cp-control-surface)]/50 dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[color:var(--cp-control-surface)]/30">
                  <p className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-[0.18em] text-[color:var(--cp-text-muted)]">
                    <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6zM14 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6zM4 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2v-2zM14 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z" />
                    </svg>
                    来源应用
                  </p>
                  <p className="mt-2 text-[13px] font-bold text-[color:var(--cp-text-primary)]">{selectedSummary.sourceApp ?? "未知来源"}</p>
                </div>
                <div className="rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[color:var(--cp-control-surface)]/30 p-4 transition-all hover:bg-[color:var(--cp-control-surface)]/50 dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[color:var(--cp-control-surface)]/30">
                  <p className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-[0.18em] text-[color:var(--cp-text-muted)]">
                    <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                    </svg>
                    最近使用
                  </p>
                  <p className="mt-2 text-[13px] font-bold text-[color:var(--cp-text-primary)]">{formatDateTime(selectedSummary.lastUsedAt)}</p>
                </div>
                <div className="rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[color:var(--cp-control-surface)]/30 p-4 transition-all hover:bg-[color:var(--cp-control-surface)]/50 dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[color:var(--cp-control-surface)]/30">
                  <p className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-[0.18em] text-[color:var(--cp-text-muted)]">
                    <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M8 7V3m8 4V3m-9 8h10M5 21h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" />
                    </svg>
                    创建时间
                  </p>
                  <p className="mt-2 text-[13px] font-bold text-[color:var(--cp-text-primary)]">{formatDateTime(selectedSummary.createdAt)}</p>
                </div>
                <div className="rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[color:var(--cp-control-surface)]/30 p-4 transition-all hover:bg-[color:var(--cp-control-surface)]/50 dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[color:var(--cp-control-surface)]/30">
                  <p className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-[0.18em] text-[color:var(--cp-text-muted)]">
                    <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                    </svg>
                    更新时间
                  </p>
                  <p className="mt-2 text-[13px] font-bold text-[color:var(--cp-text-primary)]">{formatDateTime(selectedSummary.updatedAt)}</p>
                </div>
                {/* 图片或文件类型的额外信息 */}
                {detail.data.type === "image" && (
                  <>
                    <div className="rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[color:var(--cp-control-surface)]/30 p-4 transition-all hover:bg-[color:var(--cp-control-surface)]/50 dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[color:var(--cp-control-surface)]/30">
                      <p className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-[0.18em] text-[color:var(--cp-text-muted)]">
                        <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                          <path strokeLinecap="round" strokeLinejoin="round" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
                        </svg>
                        尺寸
                      </p>
                      <p className="mt-2 text-[13px] font-bold text-[color:var(--cp-text-primary)]">
                        {detail.data.imageWidth} × {detail.data.imageHeight}
                      </p>
                    </div>
                    <div className="rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[color:var(--cp-control-surface)]/30 p-4 transition-all hover:bg-[color:var(--cp-control-surface)]/50 dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[color:var(--cp-control-surface)]/30">
                      <p className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-[0.18em] text-[color:var(--cp-text-muted)]">
                        <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                          <path strokeLinecap="round" strokeLinejoin="round" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                        </svg>
                        大小
                      </p>
                      <p className="mt-2 text-[13px] font-bold text-[color:var(--cp-text-primary)]">
                        {formatBytes(detail.data.fileSize)}
                      </p>
                    </div>
                    {detail.data.imageFormat && (
                      <div className="rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[color:var(--cp-control-surface)]/30 p-4 transition-all hover:bg-[color:var(--cp-control-surface)]/50 dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[color:var(--cp-control-surface)]/30">
                        <p className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-[0.18em] text-[color:var(--cp-text-muted)]">
                          <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                            <path strokeLinecap="round" strokeLinejoin="round" d="M10 20l4-16m4 4l4 4-4 4M6 16l-4-4 4-4" />
                          </svg>
                          格式
                        </p>
                        <p className="mt-2 text-[13px] font-bold text-[color:var(--cp-text-primary)]">{detail.data.imageFormat}</p>
                      </div>
                    )}
                  </>
                )}
                {detail.data.type === "file" && (
                  <>
                    <div className="rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[color:var(--cp-control-surface)]/30 p-4 transition-all hover:bg-[color:var(--cp-control-surface)]/50 dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[color:var(--cp-control-surface)]/30">
                      <p className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-[0.18em] text-[color:var(--cp-text-muted)]">
                        <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                          <path strokeLinecap="round" strokeLinejoin="round" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
                        </svg>
                        {getFileCountLabel(detail.data.fileCount, detail.data.directoryCount)}
                      </p>
                      <p className="mt-2 text-[13px] font-bold text-[color:var(--cp-text-primary)]">
                        {detail.data.fileCount} 个
                      </p>
                    </div>
                    <div className="rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[color:var(--cp-control-surface)]/30 p-4 transition-all hover:bg-[color:var(--cp-control-surface)]/50 dark:bg-[rgba(var(--cp-surface0-rgb),0.2)] dark:hover:bg-[color:var(--cp-control-surface)]/30">
                      <p className="flex items-center gap-1.5 text-[10px] font-bold uppercase tracking-[0.18em] text-[color:var(--cp-text-muted)]">
                        <svg className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5}>
                          <path strokeLinecap="round" strokeLinejoin="round" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                        </svg>
                        总大小
                      </p>
                      <p className="mt-2 text-[13px] font-bold text-[color:var(--cp-text-primary)]">
                        {detail.data.directoryCount > 0 ? "未统计" : formatBytes(detail.data.totalSize)}
                      </p>
                      {detail.data.directoryCount > 0 ? (
                        <p className="mt-1 text-[10px] font-medium text-[color:var(--cp-text-muted)]">包含文件夹时默认不递归统计大小。</p>
                      ) : null}
                    </div>
                  </>
                )}
              </div>

              {/* 根据类型显示内容 */}
              {detail.data.type === "text" ? (
                <>
                  <textarea
                    className="min-h-[200px] flex-1 w-full resize-none rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.3)] bg-[rgba(var(--cp-surface0-rgb),0.2)] px-5 py-5 text-[14px] leading-relaxed shadow-inner outline-none transition-all duration-300 focus:border-[rgba(var(--cp-lavender-rgb),0.4)] focus:bg-[color:var(--cp-window-shell)] focus:ring-4 focus:ring-[rgba(var(--cp-lavender-rgb),0.1)] dark:bg-[rgba(var(--cp-surface0-rgb),0.3)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
                    onChange={(event) => setDraftText(event.target.value)}
                    value={draftText}
                  />

                  <div className="flex shrink-0 flex-wrap gap-2 pt-2">
                    <button
                      className="rounded-xl border border-[rgba(var(--cp-surface1-rgb),0.4)] bg-[rgba(var(--cp-surface0-rgb),0.3)] px-5 py-2.5 text-sm font-bold text-[color:var(--cp-text-primary)] shadow-sm transition-all duration-300 hover:-translate-y-0.5 hover:border-[rgba(var(--cp-surface1-rgb),0.6)] hover:bg-[rgba(var(--cp-surface1-rgb),0.2)] active:scale-95 disabled:opacity-50"
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
                      className="rounded-xl border border-[rgba(var(--cp-red-rgb),0.3)] bg-[rgba(var(--cp-red-rgb),0.05)] px-5 py-2.5 text-sm font-bold text-[color:var(--cp-danger)] transition-all duration-300 hover:bg-[rgba(var(--cp-red-rgb),0.15)] hover:border-[rgba(var(--cp-red-rgb),0.5)] active:scale-95 disabled:opacity-50"
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
                      <div className="mt-4 w-full rounded-xl border border-[rgba(var(--cp-surface1-rgb),0.3)] bg-[rgba(var(--cp-surface0-rgb),0.2)] p-4 text-left">
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
                      className="rounded-xl border border-[rgba(var(--cp-red-rgb),0.3)] bg-[rgba(var(--cp-red-rgb),0.05)] px-5 py-2.5 text-sm font-bold text-[color:var(--cp-danger)] transition-all duration-300 hover:bg-[rgba(var(--cp-red-rgb),0.15)] hover:border-[rgba(var(--cp-red-rgb),0.5)] active:scale-95 disabled:opacity-50"
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
                <p className="shrink-0 rounded-2xl bg-[color:var(--cp-accent-warm)]/15 px-4 py-3 text-sm font-bold text-[color:var(--cp-accent-warm)] ring-1 ring-inset ring-[color:var(--cp-accent-warm)]/20 animate-in fade-in slide-in-from-bottom-2">
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
    </div>
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
          当前设置直接映射到后端持久化配置。排除应用与真正的前台应用识别将在 Windows 平台适配阶段继续细化。
        </p>
      </div>

      {errorMessage ? (
        <div className="flex items-start justify-between gap-3 rounded-2xl bg-[color:var(--cp-danger)]/15 px-4 py-3 text-sm font-bold text-[color:var(--cp-danger)] ring-1 ring-inset ring-[color:var(--cp-danger)]/20">
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
          className="w-full rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.4)] bg-[rgba(var(--cp-surface0-rgb),0.2)] px-5 py-3.5 text-sm font-medium outline-none transition-all focus:border-[rgba(var(--cp-lavender-rgb),0.4)] focus:bg-[color:var(--cp-window-shell)] focus:ring-4 focus:ring-[rgba(var(--cp-lavender-rgb),0.1)] dark:bg-[rgba(var(--cp-surface0-rgb),0.3)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
          onChange={(event) => setShortcut(event.target.value)}
          value={shortcut}
        />
      </label>

      <label className="block">
        <span className="mb-2.5 block text-[13px] font-bold text-[color:var(--cp-text-primary)]">历史记录上限</span>
        <input
          className="w-full rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.4)] bg-[rgba(var(--cp-surface0-rgb),0.2)] px-5 py-3.5 text-sm font-medium outline-none transition-all focus:border-[rgba(var(--cp-lavender-rgb),0.4)] focus:bg-[color:var(--cp-window-shell)] focus:ring-4 focus:ring-[rgba(var(--cp-lavender-rgb),0.1)] dark:bg-[rgba(var(--cp-surface0-rgb),0.3)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
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
          className="w-full rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.4)] bg-[rgba(var(--cp-surface0-rgb),0.2)] px-5 py-3.5 text-sm font-medium outline-none transition-all focus:border-[rgba(var(--cp-lavender-rgb),0.4)] focus:bg-[color:var(--cp-window-shell)] focus:ring-4 focus:ring-[rgba(var(--cp-lavender-rgb),0.1)] dark:bg-[rgba(var(--cp-surface0-rgb),0.3)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
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

      <div>
        <span className="mb-2.5 block text-[13px] font-bold text-[color:var(--cp-text-primary)]">界面主题</span>
        <div className="space-y-3">
          {themeModeOptions.map((option) => (
            <label
              className={`flex cursor-pointer items-start gap-3 rounded-2xl border px-5 py-4 transition-all duration-300 ${themeMode === option.value
                  ? "border-[rgba(var(--cp-lavender-rgb),0.4)] bg-[rgba(var(--cp-lavender-rgb),0.05)] ring-1 ring-[rgba(var(--cp-lavender-rgb),0.1)]"
                  : "border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[rgba(var(--cp-surface0-rgb),0.1)] hover:border-[rgba(var(--cp-surface1-rgb),0.4)] hover:bg-[rgba(var(--cp-surface1-rgb),0.1)]"
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
      </div>

      <div>
        <span className="mb-2.5 block text-[13px] font-bold text-[color:var(--cp-text-primary)]">速贴窗口显示位置</span>
        <div className="space-y-3">
          {pickerPositionOptions.map((option) => (
            <label
              className={`flex cursor-pointer items-start gap-3 rounded-2xl border px-5 py-4 transition-all duration-300 ${pickerPositionMode === option.value
                  ? "border-[rgba(var(--cp-lavender-rgb),0.4)] bg-[rgba(var(--cp-lavender-rgb),0.05)] ring-1 ring-[rgba(var(--cp-lavender-rgb),0.1)]"
                  : "border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[rgba(var(--cp-surface0-rgb),0.1)] hover:border-[rgba(var(--cp-surface1-rgb),0.4)] hover:bg-[rgba(var(--cp-surface1-rgb),0.1)]"
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
      </div>

      <label className="block">
        <span className="mb-2.5 block text-[13px] font-bold text-[color:var(--cp-text-primary)]">排除应用</span>
        <textarea
          className="w-full rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.4)] bg-[rgba(var(--cp-surface0-rgb),0.2)] px-5 py-3.5 text-sm font-medium leading-relaxed outline-none transition-all focus:border-[rgba(var(--cp-lavender-rgb),0.4)] focus:bg-[color:var(--cp-window-shell)] focus:ring-4 focus:ring-[rgba(var(--cp-lavender-rgb),0.1)] dark:bg-[rgba(var(--cp-surface0-rgb),0.3)] dark:focus:bg-[rgba(var(--cp-surface0-rgb),0.4)]"
          onChange={(event) => setExcludedAppsText(event.target.value)}
          placeholder={"每行一个可执行文件名，例如：\nKeePass.exe\nWindowsTerminal.exe"}
          value={excludedAppsText}
        />
      </label>

      <div className="space-y-3">
        <label className="flex cursor-pointer items-center gap-3 rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[rgba(var(--cp-surface0-rgb),0.2)] px-5 py-4 transition-all duration-300 hover:border-[rgba(var(--cp-surface1-rgb),0.4)]/30 hover:bg-[rgba(var(--cp-surface1-rgb),0.3)]">
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
          className={`flex items-center gap-3 rounded-2xl border px-5 py-4 transition-all duration-300 ${launchOnStartup
              ? "cursor-pointer border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[rgba(var(--cp-surface0-rgb),0.2)] hover:border-[rgba(var(--cp-surface1-rgb),0.4)]/30 hover:bg-[rgba(var(--cp-surface1-rgb),0.3)]"
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

        <label className="flex cursor-pointer items-center gap-3 rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[rgba(var(--cp-surface0-rgb),0.2)] px-5 py-4 transition-all duration-300 hover:border-[rgba(var(--cp-surface1-rgb),0.4)]/30 hover:bg-[rgba(var(--cp-surface1-rgb),0.3)]">
          <input
            className="h-4 w-4 rounded border-[rgba(var(--cp-surface1-rgb),0.4)] bg-cp-base text-[color:var(--cp-accent-primary)] focus:ring-[color:var(--cp-accent-primary)] dark:bg-[color:var(--cp-control-surface)]"
            checked={restoreClipboardAfterPaste}
            onChange={(event) => setRestoreClipboardAfterPaste(event.target.checked)}
            type="checkbox"
          />
          <span className="text-sm font-bold text-[color:var(--cp-text-secondary)]">回贴后恢复原始剪贴板</span>
        </label>

        <label className="flex cursor-pointer items-center gap-3 rounded-2xl border border-[rgba(var(--cp-surface1-rgb),0.2)] bg-[rgba(var(--cp-surface0-rgb),0.2)] px-5 py-4 transition-all duration-300 hover:border-[rgba(var(--cp-surface1-rgb),0.4)]/30 hover:bg-[rgba(var(--cp-surface1-rgb),0.3)]">
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
          className="rounded-2xl bg-[color:var(--cp-accent-primary)] px-8 py-4 text-sm font-bold text-cp-base shadow-lg shadow-[color:var(--cp-accent-primary)]/20 transition-all hover:brightness-110 hover:shadow-xl active:scale-95 disabled:opacity-50"
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
