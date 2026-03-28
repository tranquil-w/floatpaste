import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  SETTINGS_CHANGED_EVENT,
  MANAGER_OPEN_SETTINGS_EVENT,
} from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import { hideCurrentWindow } from "../../bridge/window";
import { queryClient } from "../../app/queryClient";
import type { PickerPositionMode, ThemeMode, UserSetting } from "../../shared/types/settings";
import { getErrorMessage } from "../../shared/utils/error";
import { useSettingsQuery, useUpdateSettingsMutation } from "./queries";

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
    description: "中性冷色调浅色主题，适合日常办公。",
  },
  {
    value: "dark",
    label: "深色",
    description: "中性深色主题，适合夜间使用。",
  },
];

const FORM_INPUT =
  "w-full rounded-md border border-pg-border-default bg-pg-canvas-inset px-4 py-2.5 text-sm outline-none transition-colors placeholder:text-pg-fg-subtle focus:border-pg-accent-fg focus:ring-1 focus:ring-pg-accent-fg focus-visible:outline-none";

const FORM_LABEL = "mb-1.5 block text-sm font-medium text-pg-fg-default";

const FORM_HINT = "mt-1.5 text-xs leading-relaxed text-pg-fg-subtle";

const SECTION_HEADING = "text-sm font-semibold text-pg-fg-default border-b border-pg-border-subtle pb-2";

export function ManagerShell() {
  const settings = useSettingsQuery();
  const updateSettingsMutation = useUpdateSettingsMutation();

  const { data } = settings;

  const [shortcut, setShortcut] = useState("Alt+Q");
  const [launchOnStartup, setLaunchOnStartup] = useState(false);
  const [silentOnStartup, setSilentOnStartup] = useState(false);
  const [historyLimit, setHistoryLimit] = useState(1000);
  const [pickerRecordLimit, setPickerRecordLimit] = useState(50);
  const [pickerPositionMode, setPickerPositionMode] = useState<PickerPositionMode>("mouse");
  const [restoreClipboardAfterPaste, setRestoreClipboardAfterPaste] = useState(true);
  const [pauseMonitoring, setPauseMonitoring] = useState(false);
  const [themeMode, setThemeMode] = useState<ThemeMode>("system");
  const [excludedAppsText, setExcludedAppsText] = useState("");
  const [workbenchShortcut, setWorkbenchShortcut] = useState("Alt+S");
  const [workbenchShortcutEnabled, setWorkbenchShortcutEnabled] = useState(true);

  useEffect(() => {
    if (!data) return;
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

  useEffect(() => {
    if (!isTauriRuntime()) return;

    let offSettings: (() => void) | undefined;
    let offOpenSettings: (() => void) | undefined;

    void listen(SETTINGS_CHANGED_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }).then((cleanup) => {
      offSettings = cleanup;
    });

    void listen(MANAGER_OPEN_SETTINGS_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }).then((cleanup) => {
      offOpenSettings = cleanup;
    });

    return () => {
      offSettings?.();
      offOpenSettings?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void hideCurrentWindow().catch(console.error);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  const saveError = updateSettingsMutation.error
    ? getErrorMessage(updateSettingsMutation.error, "保存设置失败，请稍后重试。")
    : null;

  if (settings.isLoading && !data) {
    return (
      <div className="flex h-screen items-center justify-center text-sm text-pg-fg-subtle">
        正在加载设置...
      </div>
    );
  }

  return (
    <main className="flex min-h-screen flex-col">
      <div className="mx-auto w-full max-w-[680px] px-6 py-8">
        {/* Header */}
        <div className="mb-8">
          <h1 className="text-xl font-semibold text-pg-fg-default">
            FloatPaste
          </h1>
          <p className="mt-1 text-sm text-pg-fg-muted">
            偏好设置会自动保存。
          </p>
        </div>

        {saveError ? (
          <div className="mb-6 flex items-start justify-between gap-3 rounded-md border border-pg-danger-fg bg-pg-danger-subtle px-4 py-3 text-sm text-pg-danger-fg">
            <p>{saveError}</p>
            <button
              className="shrink-0 text-xs font-semibold uppercase tracking-wider transition-opacity hover:opacity-80"
              onClick={() => updateSettingsMutation.reset()}
              type="button"
            >
              关闭
            </button>
          </div>
        ) : null}

        {/* ── 快捷键 ── */}
        <section className="mb-8">
          <h2 className={SECTION_HEADING}>快捷键</h2>
          <div className="mt-4 space-y-4">
            <label className="block">
              <span className={FORM_LABEL}>全局快捷键</span>
              <input
                className={FORM_INPUT}
                onChange={(e) => setShortcut(e.target.value)}
                value={shortcut}
              />
            </label>

            <div className="block">
              <div className="mb-1.5 flex items-center justify-between">
                <span className={FORM_LABEL}>搜索窗口快捷键</span>
                <label className="flex cursor-pointer items-center gap-2" htmlFor="workbench-shortcut-enabled">
                  <input
                    id="workbench-shortcut-enabled"
                    checked={workbenchShortcutEnabled}
                    className="h-4 w-4 rounded border-pg-border-default accent-pg-accent-fg"
                    onChange={(e) => setWorkbenchShortcutEnabled(e.target.checked)}
                    type="checkbox"
                  />
                  <span className="text-xs text-pg-fg-subtle">启用</span>
                </label>
              </div>
              <input
                className={FORM_INPUT}
                disabled={!workbenchShortcutEnabled}
                onChange={(e) => setWorkbenchShortcut(e.target.value)}
                placeholder="Alt+S"
                value={workbenchShortcut}
              />
              <p className={FORM_HINT}>全局快捷键，直接打开搜索窗口。</p>
            </div>
          </div>
        </section>

        {/* ── 通用 ── */}
        <section className="mb-8">
          <h2 className={SECTION_HEADING}>通用</h2>
          <div className="mt-4 space-y-4">
            <label className="block">
              <span className={FORM_LABEL}>历史记录上限</span>
              <input
                className={FORM_INPUT}
                min={100}
                onChange={(e) => setHistoryLimit(Number(e.target.value) || 1000)}
                step={100}
                type="number"
                value={historyLimit}
              />
            </label>

            <label className="block">
              <span className={FORM_LABEL}>速贴窗口记录数</span>
              <input
                className={FORM_INPUT}
                max={1000}
                min={9}
                onChange={(e) => setPickerRecordLimit(Number(e.target.value) || 50)}
                type="number"
                value={pickerRecordLimit}
              />
              <p className={FORM_HINT}>
                控制速贴面板一次可滚动浏览的记录数，数字快捷键仍只覆盖前 9 条。
              </p>
            </label>
          </div>
        </section>

        {/* ── 外观 ── */}
        <section className="mb-8">
          <h2 className={SECTION_HEADING}>外观</h2>
          <div className="mt-4 space-y-4">
            <fieldset className="border-0 p-0 m-0">
              <legend className={FORM_LABEL}>界面主题</legend>
              <div className="space-y-2">
                {themeModeOptions.map((option) => (
                  <label
                    className={`flex cursor-pointer items-start gap-3 rounded-md border px-4 py-3 transition-colors ${
                      themeMode === option.value
                        ? "border-pg-accent-fg bg-pg-accent-subtle"
                        : "border-pg-border-muted hover:border-pg-border-default"
                    }`}
                    key={option.value}
                  >
                    <input
                      checked={themeMode === option.value}
                      className="mt-0.5 h-4 w-4 accent-pg-accent-fg"
                      name="theme-mode"
                      onChange={() => setThemeMode(option.value)}
                      type="radio"
                    />
                    <span className="min-w-0">
                      <span className={`block text-sm font-medium ${themeMode === option.value ? "text-pg-fg-default" : "text-pg-fg-muted"}`}>
                        {option.label}
                      </span>
                      <span className="mt-0.5 block text-xs text-pg-fg-subtle">
                        {option.description}
                      </span>
                    </span>
                  </label>
                ))}
              </div>
            </fieldset>

            <fieldset className="border-0 p-0 m-0">
              <legend className={FORM_LABEL}>速贴窗口显示位置</legend>
              <div className="space-y-2">
                {pickerPositionOptions.map((option) => (
                  <label
                    className={`flex cursor-pointer items-start gap-3 rounded-md border px-4 py-3 transition-colors ${
                      pickerPositionMode === option.value
                        ? "border-pg-accent-fg bg-pg-accent-subtle"
                        : "border-pg-border-muted hover:border-pg-border-default"
                    }`}
                    key={option.value}
                  >
                    <input
                      checked={pickerPositionMode === option.value}
                      className="mt-0.5 h-4 w-4 accent-pg-accent-fg"
                      name="picker-position-mode"
                      onChange={() => setPickerPositionMode(option.value)}
                      type="radio"
                    />
                    <span className="min-w-0">
                      <span className={`block text-sm font-medium ${pickerPositionMode === option.value ? "text-pg-fg-default" : "text-pg-fg-muted"}`}>
                        {option.label}
                      </span>
                      <span className="mt-0.5 block text-xs text-pg-fg-subtle">
                        {option.description}
                      </span>
                    </span>
                  </label>
                ))}
              </div>
            </fieldset>
          </div>
        </section>

        {/* ── 行为 ── */}
        <section className="mb-8">
          <h2 className={SECTION_HEADING}>行为</h2>
          <div className="mt-4 space-y-2">
            <label className="flex cursor-pointer items-center gap-3 rounded-md border border-pg-border-muted px-4 py-3 transition-colors hover:border-pg-border-default" htmlFor="launch-on-startup">
              <input
                className="h-4 w-4 accent-pg-accent-fg"
                id="launch-on-startup"
                checked={launchOnStartup}
                onChange={(e) => {
                  const checked = e.target.checked;
                  setLaunchOnStartup(checked);
                  if (!checked) setSilentOnStartup(false);
                }}
                type="checkbox"
              />
              <span className="text-sm font-medium text-pg-fg-default">开机自启</span>
            </label>

            <label
              className={`flex items-center gap-3 rounded-md border px-4 py-3 transition-colors ${
                launchOnStartup
                  ? "cursor-pointer border-pg-border-muted hover:border-pg-border-default"
                  : "cursor-not-allowed border-pg-border-subtle"
              }`}
              htmlFor="silent-on-startup"
            >
              <input
                className="h-4 w-4 accent-pg-accent-fg"
                id="silent-on-startup"
                checked={silentOnStartup}
                disabled={!launchOnStartup}
                onChange={(e) => setSilentOnStartup(e.target.checked)}
                type="checkbox"
              />
              <span className={`text-sm font-medium ${launchOnStartup ? "text-pg-fg-default" : "text-pg-fg-subtle"}`}>
                开机时静默启动
              </span>
            </label>

            <label className="flex cursor-pointer items-center gap-3 rounded-md border border-pg-border-muted px-4 py-3 transition-colors hover:border-pg-border-default" htmlFor="restore-clipboard">
              <input
                className="h-4 w-4 accent-pg-accent-fg"
                id="restore-clipboard"
                checked={restoreClipboardAfterPaste}
                onChange={(e) => setRestoreClipboardAfterPaste(e.target.checked)}
                type="checkbox"
              />
              <span className="text-sm font-medium text-pg-fg-default">回贴后恢复剪贴板</span>
            </label>

            <label className="flex cursor-pointer items-center gap-3 rounded-md border border-pg-border-muted px-4 py-3 transition-colors hover:border-pg-border-default" htmlFor="pause-monitoring">
              <input
                className="h-4 w-4 accent-pg-accent-fg"
                id="pause-monitoring"
                checked={pauseMonitoring}
                onChange={(e) => setPauseMonitoring(e.target.checked)}
                type="checkbox"
              />
              <span className="text-sm font-medium text-pg-fg-default">暂停监听</span>
            </label>
          </div>
        </section>

        {/* ── 排除应用 ── */}
        <section className="mb-8">
          <h2 className={SECTION_HEADING}>排除应用</h2>
          <div className="mt-4">
            <label className="block">
              <textarea
                className={`${FORM_INPUT} min-h-[100px] leading-relaxed`}
                onChange={(e) => setExcludedAppsText(e.target.value)}
                placeholder={"每行一个可执行文件名，例如：\nKeePass.exe\nWindowsTerminal.exe"}
                value={excludedAppsText}
              />
            </label>
          </div>
        </section>

        {/* Save Button */}
        <div className="pt-2 pb-8">
          <button
            className="rounded-md bg-pg-accent-emphasis px-6 py-2.5 text-sm font-semibold text-pg-fg-on-emphasis transition-colors hover:bg-pg-accent-hover disabled:opacity-50"
            disabled={updateSettingsMutation.isPending}
            onClick={() => {
              updateSettingsMutation.reset();
              updateSettingsMutation.mutate({
                shortcut,
                launchOnStartup,
                silentOnStartup: launchOnStartup ? silentOnStartup : false,
                historyLimit,
                pickerRecordLimit,
                pickerPositionMode,
                themeMode,
                excludedApps: excludedAppsText
                  .split(/\r?\n/)
                  .map((v) => v.trim())
                  .filter(Boolean),
                restoreClipboardAfterPaste,
                pauseMonitoring,
                workbenchShortcut,
                workbenchShortcutEnabled,
              });
            }}
            type="button"
          >
            保存设置
          </button>
        </div>
      </div>
    </main>
  );
}

