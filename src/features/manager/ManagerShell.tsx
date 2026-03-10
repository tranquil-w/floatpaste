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
import type { SearchSort } from "../../shared/types/clips";
import type { PickerPositionMode, UserSetting } from "../../shared/types/settings";
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
    if (detail.data) {
      setDraftText(detail.data.fullText);
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

  return (
    <div className="flex h-screen flex-col px-5 py-6 text-ink md:px-8">
      <div className="mx-auto grid w-full max-w-7xl flex-1 min-h-0 gap-5 lg:grid-cols-[280px_minmax(360px,1fr)_420px]">
        <Panel className="flex flex-col gap-5 overflow-hidden">
          <div className="shrink-0">
            <div className="inline-flex items-center rounded-full border border-accent/20 bg-accent/5 px-2.5 py-1 text-[10px] font-bold uppercase tracking-[0.3em] text-accentDeep">
              FloatPaste / 浮贴
            </div>
            <h1 className="mt-3 font-display text-3xl font-medium tracking-tight">资料库窗口</h1>
            <p className="mt-3 text-sm leading-relaxed text-slate-600">
              当前版本已经打通文本记录、搜索、编辑、收藏、设置与速贴主链路，剩余工作主要集中在兼容性与稳定性收口。
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

          <div className="relative shrink-0 overflow-hidden rounded-2xl bg-gradient-to-br from-slate-900 to-slate-800 px-5 py-5 text-white shadow-lg">
            <div className="relative z-10">
              <p className="text-[10px] font-bold uppercase tracking-[0.25em] text-white/60">快捷键</p>
              <p className="mt-1.5 text-2xl font-semibold tracking-wide">{settings.data?.shortcut ?? "Ctrl+`"}</p>
              <p className="mt-3 text-[13px] leading-relaxed text-white/70">全局快捷键、速贴面板与回贴链路已经接入；可在设置中继续调整记录数、显示位置与启动行为。</p>
            </div>
            <div className="absolute -right-4 -top-4 h-24 w-24 rounded-full bg-white/5 blur-2xl"></div>
          </div>

          <div className="flex min-h-0 flex-1 flex-col">
            <div className="mb-3 flex shrink-0 items-center justify-between">
              <h2 className="text-[11px] font-bold uppercase tracking-[0.18em] text-slate-400">
                收藏预览
              </h2>
              <button
                className="text-[13px] font-medium text-accentDeep transition-colors hover:text-accent"
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
                    className="group w-full rounded-2xl border border-amber-200/50 bg-gradient-to-br from-amber-50 to-orange-50/30 px-4 py-3 text-left transition-all duration-300 hover:-translate-y-0.5 hover:border-amber-300/80 hover:shadow-[0_4px_12px_-4px_rgba(217,119,6,0.15)]"
                    key={item.id}
                    onClick={() => {
                      setViewMode("history");
                      setSelectedItemId(item.id);
                    }}
                    type="button"
                  >
                    <p className="line-clamp-2 text-sm font-medium leading-relaxed text-slate-800">{item.contentPreview}</p>
                    <p className="mt-2 text-[11px] font-medium text-amber-700/60">{formatDateTime(item.lastUsedAt ?? item.createdAt)}</p>
                  </button>
                ))
              ) : (
                <EmptyState title="还没有收藏内容" description="在中间列表里点亮星标后，这里会优先显示常用片段。" />
              )}
            </div>
          </div>

          <div className="mt-auto flex shrink-0 gap-2 pt-4">
            <button
              className="whitespace-nowrap rounded-2xl bg-gradient-to-b from-amber-400 to-amber-500 px-4 py-3 text-sm font-semibold text-white shadow-[0_2px_10px_rgba(217,119,6,0.2)] transition-all duration-300 hover:to-amber-600 hover:shadow-[0_4px_14px_rgba(217,119,6,0.3)]"
              onClick={() => void showPicker()}
              type="button"
            >
              打开速贴
            </button>
            <button
              className={`whitespace-nowrap flex-1 rounded-2xl px-4 py-3 text-sm font-semibold transition-all duration-300 ${
                viewMode === "history"
                  ? "bg-slate-900 text-white shadow-md"
                  : "bg-white text-slate-700 ring-1 ring-inset ring-slate-200 hover:bg-slate-50 hover:text-slate-900"
              }`}
              onClick={() => setViewMode("history")}
              type="button"
            >
              历史库
            </button>
            <button
              className={`whitespace-nowrap flex-1 rounded-2xl px-4 py-3 text-sm font-semibold transition-all duration-300 ${
                viewMode === "settings"
                  ? "bg-slate-900 text-white shadow-md"
                  : "bg-white text-slate-700 ring-1 ring-inset ring-slate-200 hover:bg-slate-50 hover:text-slate-900"
              }`}
              onClick={() => setViewMode("settings")}
              type="button"
            >
              设置
            </button>
          </div>
        </Panel>

        <Panel className="flex flex-col gap-4 overflow-hidden">
          {viewMode === "history" ? (
            <>
              <div className="flex shrink-0 flex-col gap-3 sm:flex-row">
                <input
                  className="flex-1 rounded-2xl border border-slate-200 bg-slate-50/50 px-5 py-3.5 text-sm outline-none transition-all focus:border-accent focus:bg-white focus:ring-4 focus:ring-accent/10"
                  onChange={(event) => setKeyword(event.target.value)}
                  placeholder="搜索全文、来源应用或关键短语"
                  value={keyword}
                />
                <label className="inline-flex cursor-pointer items-center gap-2 rounded-2xl border border-slate-200 bg-white px-5 py-3.5 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50">
                  <input
                    className="h-4 w-4 rounded border-slate-300 text-accent focus:ring-accent"
                    checked={favoritedOnly}
                    onChange={(event) => setFavoritedOnly(event.target.checked)}
                    type="checkbox"
                  />
                  只看收藏
                </label>
              </div>

              <div className="flex shrink-0 items-center justify-between px-1 text-xs font-medium text-slate-400">
                <span>共 {totalCount} 条记录</span>
                <span>{debouncedKeyword.trim() ? "按相关度排序" : "按最近活跃排序"}</span>
              </div>

              <div className="flex-1 space-y-3 overflow-y-auto pr-2 pl-1 py-1">
                {currentItems.length ? (
                  currentItems.map((item, index) => {
                    const isSelected = item.id === selectedItemId;
                    return (
                      <button
                        className={`group w-full rounded-3xl border px-5 py-4 text-left transition-all duration-300 ${
                          isSelected
                            ? "scale-[1.01] border-accent/40 bg-white shadow-[0_4px_20px_-4px_rgba(217,119,6,0.15)] ring-1 ring-accent/30"
                            : "border-slate-200/60 bg-white/60 hover:-translate-y-0.5 hover:border-slate-300 hover:bg-white hover:shadow-sm"
                        }`}
                        key={item.id}
                        onClick={() => setSelectedItemId(item.id)}
                        type="button"
                      >
                        <div className="flex items-start justify-between gap-4">
                          <div className="flex-1">
                            <div className="flex h-5 w-5 items-center justify-center rounded-md bg-slate-100/80 text-[10px] font-bold text-slate-400 transition-colors group-hover:bg-slate-200 group-hover:text-slate-500">
                              {String(searchQuery.offset + index + 1).padStart(2, "0")}
                            </div>
                            <p className="mt-2.5 line-clamp-3 text-sm leading-relaxed text-slate-800">
                              {item.contentPreview}
                            </p>
                          </div>
                          {item.isFavorited ? (
                            <span className="shrink-0 rounded-full bg-amber-100/80 px-2 py-1 text-[10px] font-semibold text-amber-700">
                              收藏
                            </span>
                          ) : null}
                        </div>
                        <div className="mt-3.5 flex flex-wrap gap-x-4 gap-y-2 text-[11px] font-medium text-slate-400">
                          <span className="flex items-center gap-1.5"><span className="h-1 w-1 rounded-full bg-slate-300"></span>{item.sourceApp ?? "未知来源"}</span>
                          <span className="flex items-center gap-1.5"><span className="h-1 w-1 rounded-full bg-slate-300"></span>创建于 {formatDateTime(item.createdAt)}</span>
                          <span className="flex items-center gap-1.5"><span className="h-1 w-1 rounded-full bg-slate-300"></span>最近使用 {formatDateTime(item.lastUsedAt)}</span>
                        </div>
                      </button>
                    );
                  })
                ) : (
                  <EmptyState
                    title="当前没有匹配结果"
                    description="复制任意文本后会自动入库；如果在浏览器预览模式中运行，这里会展示模拟数据。"
                  />
                )}
              </div>

              <div className="flex shrink-0 items-center justify-between gap-3 border-t border-slate-200/80 px-1 pt-4 text-xs font-medium text-slate-400">
                <span>{totalCount === 0 ? "当前没有可显示记录" : `当前显示第 ${pageStart}-${pageEnd} 条`}</span>
                <div className="flex items-center gap-2">
                  <button
                    className="rounded-xl bg-white px-3 py-2 text-xs font-semibold text-slate-700 shadow-sm ring-1 ring-inset ring-slate-200 transition-all hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-40"
                    disabled={!hasPreviousPage}
                    onClick={() => setPageIndex((current) => Math.max(current - 1, 0))}
                    type="button"
                  >
                    上一页
                  </button>
                  <button
                    className="rounded-xl bg-white px-3 py-2 text-xs font-semibold text-slate-700 shadow-sm ring-1 ring-inset ring-slate-200 transition-all hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-40"
                    disabled={!hasNextPage}
                    onClick={() => setPageIndex((current) => current + 1)}
                    type="button"
                  >
                    下一页
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

        <Panel className="flex flex-col gap-4 overflow-hidden">
          {detail.data && selectedSummary ? (
            <>
              <div className="flex shrink-0 flex-wrap items-center justify-between gap-3">
                <div>
                  <p className="text-[11px] font-bold uppercase tracking-[0.22em] text-slate-400">
                    详情编辑
                  </p>
                  <h2 className="mt-1 font-display text-2xl font-medium tracking-tight">文本剪贴项</h2>
                </div>
                <div className="flex gap-2">
                  <button
                    className="rounded-xl bg-white px-4 py-2 text-sm font-medium text-slate-700 shadow-sm ring-1 ring-inset ring-slate-200 transition-all hover:bg-slate-50 hover:text-slate-900"
                    onClick={() =>
                      favoritedMutation.mutate({
                        id: detail.data.id,
                        value: !detail.data.isFavorited,
                      })
                    }
                    type="button"
                  >
                    {detail.data.isFavorited ? "取消收藏" : "加入收藏"}
                  </button>
                  <button
                    className="rounded-xl bg-gradient-to-b from-amber-400 to-amber-500 px-4 py-2 text-sm font-semibold text-white shadow-sm transition-all hover:to-amber-600 hover:shadow"
                    onClick={() =>
                      pasteMutation.mutate({
                        id: detail.data.id,
                        option: {
                          restoreClipboardAfterPaste:
                            settings.data?.restoreClipboardAfterPaste ?? true,
                        },
                      })
                    }
                    type="button"
                  >
                    写入剪贴板
                  </button>
                </div>
              </div>

              <div className="grid shrink-0 gap-3 rounded-2xl border border-slate-100 bg-slate-50/50 p-5 text-sm text-slate-600 sm:grid-cols-2">
                <div>
                  <p className="text-[10px] font-bold uppercase tracking-[0.18em] text-slate-400">来源应用</p>
                  <p className="mt-1.5 font-medium text-slate-800">{selectedSummary.sourceApp ?? "未知来源"}</p>
                </div>
                <div>
                  <p className="text-[10px] font-bold uppercase tracking-[0.18em] text-slate-400">最近使用</p>
                  <p className="mt-1.5 font-medium text-slate-800">{formatDateTime(selectedSummary.lastUsedAt)}</p>
                </div>
                <div>
                  <p className="text-[10px] font-bold uppercase tracking-[0.18em] text-slate-400">创建时间</p>
                  <p className="mt-1.5 font-medium text-slate-800">{formatDateTime(selectedSummary.createdAt)}</p>
                </div>
                <div>
                  <p className="text-[10px] font-bold uppercase tracking-[0.18em] text-slate-400">更新时间</p>
                  <p className="mt-1.5 font-medium text-slate-800">{formatDateTime(selectedSummary.updatedAt)}</p>
                </div>
              </div>

              <textarea
                className="min-h-[200px] flex-1 w-full resize-none rounded-2xl border border-slate-200 bg-slate-50/50 px-5 py-5 text-[15px] leading-relaxed outline-none transition-all duration-300 focus:border-accent focus:bg-white focus:ring-4 focus:ring-accent/10"
                onChange={(event) => setDraftText(event.target.value)}
                value={draftText}
              />

              <div className="flex shrink-0 flex-wrap gap-2 pt-2">
                <button
                  className="rounded-2xl bg-slate-900 px-5 py-3 text-sm font-semibold text-white shadow-md transition-all hover:bg-slate-800 hover:shadow-lg disabled:opacity-50"
                  disabled={updateTextMutation.isPending}
                  onClick={() =>
                    updateTextMutation.mutate({
                      id: detail.data.id,
                      text: draftText,
                    })
                  }
                  type="button"
                >
                  保存文本
                </button>
                <button
                  className="rounded-2xl bg-white px-5 py-3 text-sm font-semibold text-red-600 shadow-sm ring-1 ring-inset ring-red-200 transition-all hover:bg-red-50 hover:text-red-700 disabled:opacity-50"
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

              {pasteMutation.data ? (
                <p className="shrink-0 rounded-2xl bg-amber-50/80 px-4 py-3 text-sm text-amber-800 ring-1 ring-inset ring-amber-500/20">
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
    setExcludedAppsText(data.excludedApps.join("\n"));
  }, [data]);

  if (isLoading && !data) {
    return <EmptyState title="正在加载设置" description="稍后即可编辑快捷键、历史上限和排除应用。" />;
  }

  return (
    <div className="space-y-6">
      <div>
        <p className="text-[11px] font-bold uppercase tracking-[0.22em] text-slate-400">设置</p>
          <h2 className="mt-1 font-display text-2xl font-medium tracking-tight">运行设置</h2>
        <p className="mt-3 text-sm leading-relaxed text-slate-600">
          当前设置直接映射到后端持久化配置。排除应用与真正的前台应用识别将在 Windows 平台适配阶段继续细化。
        </p>
      </div>

      {errorMessage ? (
        <div className="flex items-start justify-between gap-3 rounded-2xl bg-red-50/90 px-4 py-3 text-sm text-red-700 ring-1 ring-inset ring-red-200">
          <p className="leading-relaxed">{errorMessage}</p>
          <button
            className="shrink-0 text-xs font-semibold text-red-700 transition-colors hover:text-red-900"
            onClick={onDismissError}
            type="button"
          >
            关闭
          </button>
        </div>
      ) : null}

      <label className="block">
        <span className="mb-2.5 block text-sm font-medium text-slate-700">全局快捷键</span>
        <input
          className="w-full rounded-2xl border border-slate-200 bg-slate-50/50 px-5 py-3.5 text-sm outline-none transition-all focus:border-accent focus:bg-white focus:ring-4 focus:ring-accent/10"
          onChange={(event) => setShortcut(event.target.value)}
          value={shortcut}
        />
      </label>

      <label className="block">
        <span className="mb-2.5 block text-sm font-medium text-slate-700">历史记录上限</span>
        <input
          className="w-full rounded-2xl border border-slate-200 bg-slate-50/50 px-5 py-3.5 text-sm outline-none transition-all focus:border-accent focus:bg-white focus:ring-4 focus:ring-accent/10"
          min={100}
          onChange={(event) => setHistoryLimit(Number(event.target.value) || 1000)}
          step={100}
          type="number"
          value={historyLimit}
        />
      </label>

      <label className="block">
        <span className="mb-2.5 block text-sm font-medium text-slate-700">速贴窗口记录数</span>
        <input
          className="w-full rounded-2xl border border-slate-200 bg-slate-50/50 px-5 py-3.5 text-sm outline-none transition-all focus:border-accent focus:bg-white focus:ring-4 focus:ring-accent/10"
          max={1000}
          min={9}
          onChange={(event) => setPickerRecordLimit(Number(event.target.value) || 50)}
          type="number"
          value={pickerRecordLimit}
        />
        <p className="mt-2 text-xs leading-relaxed text-slate-500">
          控制速贴面板一次可滚动浏览的记录数，数字快捷键仍只覆盖前 9 条。
        </p>
      </label>

      <div>
        <span className="mb-2.5 block text-sm font-medium text-slate-700">速贴窗口显示位置</span>
        <div className="space-y-3">
          {pickerPositionOptions.map((option) => (
            <label
              className="flex cursor-pointer items-start gap-3 rounded-2xl border border-slate-200 bg-white px-5 py-4 transition-colors hover:bg-slate-50"
              key={option.value}
            >
              <input
                checked={pickerPositionMode === option.value}
                className="mt-0.5 h-4 w-4 border-slate-300 text-accent focus:ring-accent"
                name="picker-position-mode"
                onChange={() => setPickerPositionMode(option.value)}
                type="radio"
              />
              <span className="min-w-0">
                <span className="block text-sm font-medium text-slate-700">{option.label}</span>
                <span className="mt-1 block text-xs leading-relaxed text-slate-500">
                  {option.description}
                </span>
              </span>
            </label>
          ))}
        </div>
      </div>

      <label className="block">
        <span className="mb-2.5 block text-sm font-medium text-slate-700">排除应用</span>
        <textarea
          className="min-h-[120px] w-full rounded-2xl border border-slate-200 bg-slate-50/50 px-5 py-3.5 text-sm leading-relaxed outline-none transition-all focus:border-accent focus:bg-white focus:ring-4 focus:ring-accent/10"
          onChange={(event) => setExcludedAppsText(event.target.value)}
          placeholder={"每行一个可执行文件名，例如：\nKeePass.exe\nWindowsTerminal.exe"}
          value={excludedAppsText}
        />
      </label>

      <div className="space-y-3">
        <label className="flex cursor-pointer items-center gap-3 rounded-2xl border border-slate-200 bg-white px-5 py-4 transition-colors hover:bg-slate-50">
          <input
            className="h-4 w-4 rounded border-slate-300 text-accent focus:ring-accent"
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
          <span className="text-sm font-medium text-slate-700">开机自启</span>
        </label>

        <label
          className={`flex items-center gap-3 rounded-2xl border px-5 py-4 transition-colors ${
            launchOnStartup
              ? "cursor-pointer border-slate-200 bg-white hover:bg-slate-50"
              : "cursor-not-allowed border-slate-100 bg-slate-50 text-slate-400"
          }`}
        >
          <input
            className="h-4 w-4 rounded border-slate-300 text-accent focus:ring-accent"
            checked={silentOnStartup}
            disabled={!launchOnStartup}
            onChange={(event) => setSilentOnStartup(event.target.checked)}
            type="checkbox"
          />
          <span className="text-sm font-medium text-slate-700">开机自启时静默启动</span>
        </label>

        <label className="flex cursor-pointer items-center gap-3 rounded-2xl border border-slate-200 bg-white px-5 py-4 transition-colors hover:bg-slate-50">
          <input
            className="h-4 w-4 rounded border-slate-300 text-accent focus:ring-accent"
            checked={restoreClipboardAfterPaste}
            onChange={(event) => setRestoreClipboardAfterPaste(event.target.checked)}
            type="checkbox"
          />
          <span className="text-sm font-medium text-slate-700">回贴后恢复原始剪贴板</span>
        </label>

        <label className="flex cursor-pointer items-center gap-3 rounded-2xl border border-slate-200 bg-white px-5 py-4 transition-colors hover:bg-slate-50">
          <input
            className="h-4 w-4 rounded border-slate-300 text-accent focus:ring-accent"
            checked={pauseMonitoring}
            onChange={(event) => setPauseMonitoring(event.target.checked)}
            type="checkbox"
          />
          <span className="text-sm font-medium text-slate-700">暂停监听</span>
        </label>
      </div>

      <div className="pt-2">
        <button
          className="rounded-2xl bg-slate-900 px-6 py-3.5 text-sm font-semibold text-white shadow-md transition-all hover:bg-slate-800 hover:shadow-lg disabled:opacity-50"
          disabled={isPending}
          onClick={() =>
            onSave({
              shortcut,
              launchOnStartup,
              silentOnStartup: launchOnStartup ? silentOnStartup : false,
              historyLimit,
              pickerRecordLimit,
              pickerPositionMode,
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
