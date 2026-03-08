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
import { queryClient } from "../../app/queryClient";
import type { SearchSort } from "../../shared/types/clips";
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

export function ManagerShell() {
  const { selectedItemId, draftText, viewMode, setDraftText, setSelectedItemId, setViewMode } =
    useManagerStore();
  const [keyword, setKeyword] = useState("");
  const [favoritedOnly, setFavoritedOnly] = useState(false);
  const searchQuery = useMemo(
    () => ({
      keyword,
      filters: {
        favoritedOnly,
      },
      offset: 0,
      limit: 50,
      sort: (keyword.trim() ? "relevance_desc" : "recent_desc") as SearchSort,
    }),
    [favoritedOnly, keyword],
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
    if (!selectedItemId || !clips.data) {
      return;
    }

    const existsInCurrentResult = clips.data.items.some((item) => item.id === selectedItemId);
    if (!existsInCurrentResult) {
      setSelectedItemId(null);
    }
  }, [clips.data, selectedItemId, setSelectedItemId]);

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
    if (detail.data) {
      setDraftText(detail.data.fullText);
    }
  }, [detail.data, setDraftText]);

  const selectedSummary = clips.data?.items.find((item) => item.id === selectedItemId) ?? detail.data;

  return (
    <div className="min-h-screen px-5 py-6 text-ink md:px-8">
      <div className="mx-auto grid max-w-7xl gap-5 lg:grid-cols-[280px_minmax(360px,1fr)_420px]">
        <Panel className="flex flex-col gap-5">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.3em] text-accentDeep">
              FloatPaste / 浮贴
            </p>
            <h1 className="mt-2 font-display text-3xl leading-tight">MVP 资料库窗口</h1>
            <p className="mt-3 text-sm leading-6 text-slate-600">
              这一版优先打通文本记录、搜索、编辑、收藏和设置，速贴面板与系统回贴链路继续沿当前骨架推进。
            </p>
          </div>

          <div className="flex flex-wrap gap-2">
            <StatusBadge tone={settings.data?.pauseMonitoring ? "paused" : "running"}>
              {settings.data?.pauseMonitoring ? "监听已暂停" : "监听中"}
            </StatusBadge>
            <StatusBadge tone="muted">
              {`收藏 ${favorites.data?.length ?? 0}`}
            </StatusBadge>
          </div>

          <div className="rounded-2xl bg-slate-900 px-4 py-4 text-white">
            <p className="text-xs uppercase tracking-[0.25em] text-white/70">快捷键</p>
            <p className="mt-2 text-2xl font-semibold">{settings.data?.shortcut ?? "Ctrl+`"}</p>
            <p className="mt-2 text-sm text-white/70">当前仅完成 Manager 主窗口，Picker 与全局唤起链路在下一阶段接入。</p>
          </div>

          <div>
            <div className="mb-3 flex items-center justify-between">
              <h2 className="text-sm font-semibold uppercase tracking-[0.18em] text-slate-500">
                收藏预览
              </h2>
              <button
                className="text-sm font-medium text-accentDeep"
                onClick={() => setViewMode("history")}
                type="button"
              >
                查看全部
              </button>
            </div>
            <div className="space-y-3">
              {favorites.data?.length ? (
                favorites.data.map((item) => (
                  <button
                    className="w-full rounded-2xl border border-amber-100 bg-amber-50 px-4 py-3 text-left transition hover:-translate-y-0.5 hover:border-amber-300"
                    key={item.id}
                    onClick={() => {
                      setViewMode("history");
                      setSelectedItemId(item.id);
                    }}
                    type="button"
                  >
                    <p className="line-clamp-2 text-sm font-medium">{item.contentPreview}</p>
                    <p className="mt-2 text-xs text-slate-500">{formatDateTime(item.lastUsedAt ?? item.createdAt)}</p>
                  </button>
                ))
              ) : (
                <EmptyState title="还没有收藏内容" description="在中间列表里点亮星标后，这里会优先显示常用片段。" />
              )}
            </div>
          </div>

          <div className="mt-auto flex gap-2">
            <button
              className="rounded-2xl bg-amber-500 px-4 py-3 text-sm font-semibold text-white transition hover:bg-amber-600"
              onClick={() => void showPicker()}
              type="button"
            >
              打开速贴
            </button>
            <button
              className={`flex-1 rounded-2xl px-4 py-3 text-sm font-semibold transition ${
                viewMode === "history"
                  ? "bg-ink text-white"
                  : "bg-white text-slate-700 ring-1 ring-slate-200"
              }`}
              onClick={() => setViewMode("history")}
              type="button"
            >
              历史库
            </button>
            <button
              className={`flex-1 rounded-2xl px-4 py-3 text-sm font-semibold transition ${
                viewMode === "settings"
                  ? "bg-ink text-white"
                  : "bg-white text-slate-700 ring-1 ring-slate-200"
              }`}
              onClick={() => setViewMode("settings")}
              type="button"
            >
              设置
            </button>
          </div>
        </Panel>

        <Panel className="flex flex-col gap-4">
          {viewMode === "history" ? (
            <>
              <div className="flex flex-col gap-3 sm:flex-row">
                <input
                  className="flex-1 rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3 outline-none transition focus:border-accent focus:bg-white"
                  onChange={(event) => setKeyword(event.target.value)}
                  placeholder="搜索全文、来源应用或关键短语"
                  value={keyword}
                />
                <label className="inline-flex items-center gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm text-slate-700">
                  <input
                    checked={favoritedOnly}
                    onChange={(event) => setFavoritedOnly(event.target.checked)}
                    type="checkbox"
                  />
                  只看收藏
                </label>
              </div>

              <div className="flex items-center justify-between text-sm text-slate-500">
                <span>共 {clips.data?.total ?? 0} 条记录</span>
                <span>{keyword.trim() ? "按相关度排序" : "按最近使用排序"}</span>
              </div>

              <div className="space-y-3 overflow-y-auto pr-1">
                {clips.data?.items.length ? (
                  clips.data.items.map((item, index) => {
                    const isSelected = item.id === selectedItemId;
                    return (
                      <button
                        className={`w-full rounded-3xl border px-4 py-4 text-left transition ${
                          isSelected
                            ? "border-accent bg-amber-50 shadow-sm"
                            : "border-slate-200 bg-white hover:-translate-y-0.5 hover:border-slate-300"
                        }`}
                        key={item.id}
                        onClick={() => setSelectedItemId(item.id)}
                        type="button"
                      >
                        <div className="flex items-start justify-between gap-4">
                          <div>
                            <p className="text-xs font-semibold uppercase tracking-[0.22em] text-slate-400">
                              {String(index + 1).padStart(2, "0")}
                            </p>
                            <p className="mt-2 line-clamp-3 text-sm leading-6 text-slate-800">
                              {item.contentPreview}
                            </p>
                          </div>
                          {item.isFavorited ? (
                            <span className="rounded-full bg-amber-100 px-2 py-1 text-xs font-semibold text-amber-700">
                              收藏
                            </span>
                          ) : null}
                        </div>
                        <div className="mt-3 flex flex-wrap gap-3 text-xs text-slate-500">
                          <span>{item.sourceApp ?? "未知来源"}</span>
                          <span>创建于 {formatDateTime(item.createdAt)}</span>
                          <span>最近使用 {formatDateTime(item.lastUsedAt)}</span>
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
            </>
          ) : (
            <SettingsPanel
              isPending={updateSettingsMutation.isPending}
              onSave={(nextValue) => updateSettingsMutation.mutate(nextValue)}
            />
          )}
        </Panel>

        <Panel className="flex flex-col gap-4">
          {detail.data && selectedSummary ? (
            <>
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <p className="text-xs font-semibold uppercase tracking-[0.22em] text-slate-500">
                    详情编辑
                  </p>
                  <h2 className="mt-2 font-display text-2xl">文本剪贴项</h2>
                </div>
                <div className="flex gap-2">
                  <button
                    className="rounded-2xl bg-slate-900 px-4 py-2 text-sm font-semibold text-white transition hover:bg-slate-700"
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
                    className="rounded-2xl bg-amber-500 px-4 py-2 text-sm font-semibold text-white transition hover:bg-amber-600"
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

              <div className="grid gap-3 rounded-3xl bg-slate-50 p-4 text-sm text-slate-600 sm:grid-cols-2">
                <div>
                  <p className="text-xs uppercase tracking-[0.18em] text-slate-400">来源应用</p>
                  <p className="mt-1 font-medium text-slate-800">{selectedSummary.sourceApp ?? "未知来源"}</p>
                </div>
                <div>
                  <p className="text-xs uppercase tracking-[0.18em] text-slate-400">最近使用</p>
                  <p className="mt-1 font-medium text-slate-800">{formatDateTime(selectedSummary.lastUsedAt)}</p>
                </div>
                <div>
                  <p className="text-xs uppercase tracking-[0.18em] text-slate-400">创建时间</p>
                  <p className="mt-1 font-medium text-slate-800">{formatDateTime(selectedSummary.createdAt)}</p>
                </div>
                <div>
                  <p className="text-xs uppercase tracking-[0.18em] text-slate-400">更新时间</p>
                  <p className="mt-1 font-medium text-slate-800">{formatDateTime(selectedSummary.updatedAt)}</p>
                </div>
              </div>

              <textarea
                className="min-h-[320px] w-full rounded-3xl border border-slate-200 bg-slate-50 px-4 py-4 leading-7 outline-none transition focus:border-accent focus:bg-white"
                onChange={(event) => setDraftText(event.target.value)}
                value={draftText}
              />

              <div className="flex flex-wrap gap-2">
                <button
                  className="rounded-2xl bg-ink px-4 py-3 text-sm font-semibold text-white transition hover:bg-slate-700"
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
                  className="rounded-2xl bg-white px-4 py-3 text-sm font-semibold text-red-600 ring-1 ring-red-200 transition hover:bg-red-50"
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
                <p className="rounded-2xl bg-amber-50 px-4 py-3 text-sm text-amber-800">
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
  isPending: boolean;
  onSave: (payload: {
    shortcut: string;
    launchOnStartup: boolean;
    historyLimit: number;
    excludedApps: string[];
    restoreClipboardAfterPaste: boolean;
    pauseMonitoring: boolean;
  }) => void;
}

function SettingsPanel({ isPending, onSave }: SettingsPanelProps) {
  const { data, isLoading } = useSettingsQuery();
  const [shortcut, setShortcut] = useState("Ctrl+`");
  const [historyLimit, setHistoryLimit] = useState(1000);
  const [restoreClipboardAfterPaste, setRestoreClipboardAfterPaste] = useState(true);
  const [pauseMonitoring, setPauseMonitoring] = useState(false);
  const [excludedAppsText, setExcludedAppsText] = useState("");

  useEffect(() => {
    if (!data) {
      return;
    }

    setShortcut(data.shortcut);
    setHistoryLimit(data.historyLimit);
    setRestoreClipboardAfterPaste(data.restoreClipboardAfterPaste);
    setPauseMonitoring(data.pauseMonitoring);
    setExcludedAppsText(data.excludedApps.join("\n"));
  }, [data]);

  if (isLoading && !data) {
    return <EmptyState title="正在加载设置" description="稍后即可编辑快捷键、历史上限和排除应用。" />;
  }

  return (
    <div className="space-y-5">
      <div>
        <p className="text-xs font-semibold uppercase tracking-[0.22em] text-slate-500">设置</p>
        <h2 className="mt-2 font-display text-2xl">MVP 运行参数</h2>
        <p className="mt-2 text-sm leading-6 text-slate-600">
          当前设置直接映射到后端持久化配置。排除应用与真正的前台应用识别将在 Windows 平台适配阶段继续细化。
        </p>
      </div>

      <label className="block">
        <span className="mb-2 block text-sm font-medium text-slate-700">全局快捷键</span>
        <input
          className="w-full rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3"
          onChange={(event) => setShortcut(event.target.value)}
          value={shortcut}
        />
      </label>

      <label className="block">
        <span className="mb-2 block text-sm font-medium text-slate-700">历史记录上限</span>
        <input
          className="w-full rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3"
          min={100}
          onChange={(event) => setHistoryLimit(Number(event.target.value) || 1000)}
          step={100}
          type="number"
          value={historyLimit}
        />
      </label>

      <label className="block">
        <span className="mb-2 block text-sm font-medium text-slate-700">排除应用</span>
        <textarea
          className="min-h-[120px] w-full rounded-2xl border border-slate-200 bg-slate-50 px-4 py-3"
          onChange={(event) => setExcludedAppsText(event.target.value)}
          placeholder={"每行一个可执行文件名，例如：\nKeePass.exe\nWindowsTerminal.exe"}
          value={excludedAppsText}
        />
      </label>

      <label className="flex items-center gap-3 rounded-2xl border border-slate-200 bg-white px-4 py-3">
        <input
          checked={restoreClipboardAfterPaste}
          onChange={(event) => setRestoreClipboardAfterPaste(event.target.checked)}
          type="checkbox"
        />
        回贴后恢复原始剪贴板
      </label>

      <label className="flex items-center gap-3 rounded-2xl border border-slate-200 bg-white px-4 py-3">
        <input
          checked={pauseMonitoring}
          onChange={(event) => setPauseMonitoring(event.target.checked)}
          type="checkbox"
        />
        暂停监听
      </label>

      <button
        className="rounded-2xl bg-ink px-4 py-3 text-sm font-semibold text-white transition hover:bg-slate-700"
        disabled={isPending}
        onClick={() =>
          onSave({
            shortcut,
            launchOnStartup: false,
            historyLimit,
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
  );
}
