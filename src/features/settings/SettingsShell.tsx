import { useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  SETTINGS_CHANGED_EVENT,
  SETTINGS_OPEN_SETTINGS_EVENT,
} from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import { hideCurrentWindow } from "../../bridge/window";
import { queryClient } from "../../app/queryClient";
import type {
  CustomThemeColors,
  PickerPositionMode,
  ThemeColorPalette,
  ThemeMode,
  UserSetting,
} from "../../shared/types/settings";
import {
  DEFAULT_CUSTOM_THEME_COLORS,
  getCustomThemeColorErrors,
  sanitizeCustomThemeColors,
} from "../../shared/themeColors";
import { LoadingSpinner } from "../../shared/ui/LoadingSpinner";
import { getErrorMessage } from "../../shared/utils/error";
import { SettingsNav } from "./SettingsNav";
import { SettingsSection } from "./SettingsSection";
import { SETTINGS_SECTIONS, type SettingsSectionId } from "./settingsSections";
import { useSettingsNavigation } from "./useSettingsNavigation";
import { useSettingsQuery, useUpdateSettingsMutation } from "./queries";

type EditableSettings = {
  shortcut: string;
  launchOnStartup: boolean;
  silentOnStartup: boolean;
  historyLimit: number;
  pickerRecordLimit: number;
  pickerPositionMode: PickerPositionMode;
  restoreClipboardAfterPaste: boolean;
  pauseMonitoring: boolean;
  themeMode: ThemeMode;
  excludedAppsText: string;
  searchShortcut: string;
  searchShortcutEnabled: boolean;
  customThemeColors: CustomThemeColors;
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
    description: "中性浅色基底，跨设备观感更稳定。",
  },
  {
    value: "dark",
    label: "深色",
    description: "冷调深色主题，适合夜间使用。",
  },
];

const sectionDescriptions: Record<SettingsSectionId, string> = {
  shortcuts: "配置全局唤起与搜索入口，保持高频操作一眼可见。",
  general: "调整历史容量与速贴列表承载范围，平衡性能和浏览密度。",
  appearance: "管理界面主题与速贴窗口出现方式，保证日常使用手感一致。",
  behavior: "控制开机启动、监听状态与贴回行为，明确主次关系。",
  excludedApps: "按进程名忽略特定应用，避免敏感内容进入历史记录。",
};

const FORM_INPUT =
  "w-full rounded-xl border border-pg-border-default bg-pg-canvas-default px-4 py-2.5 text-sm outline-none transition-colors placeholder:text-pg-fg-subtle focus:border-pg-accent-fg focus:ring-1 focus:ring-pg-accent-fg focus-visible:outline-none disabled:cursor-not-allowed disabled:border-pg-border-subtle disabled:bg-pg-canvas-subtle disabled:text-pg-fg-subtle";

const FORM_LABEL = "mb-1.5 block text-sm font-medium text-pg-fg-default";
const FORM_HINT = "mt-1.5 text-xs leading-relaxed text-pg-fg-subtle";
const CARD_CLASS =
  "rounded-2xl border border-pg-border-muted bg-pg-canvas-subtle px-5 py-5 shadow-sm";

function toEditableSettings(settings: UserSetting): EditableSettings {
  return {
    shortcut: settings.shortcut,
    launchOnStartup: settings.launchOnStartup,
    silentOnStartup: settings.silentOnStartup,
    historyLimit: settings.historyLimit,
    pickerRecordLimit: settings.pickerRecordLimit,
    pickerPositionMode: settings.pickerPositionMode,
    restoreClipboardAfterPaste: settings.restoreClipboardAfterPaste,
    pauseMonitoring: settings.pauseMonitoring,
    themeMode: settings.themeMode,
    excludedAppsText: settings.excludedApps.join("\n"),
    searchShortcut: settings.searchShortcut,
    searchShortcutEnabled: settings.searchShortcutEnabled,
    customThemeColors: settings.customThemeColors,
  };
}

function toSettingsPayload(editable: EditableSettings): UserSetting {
  return {
    shortcut: editable.shortcut,
    launchOnStartup: editable.launchOnStartup,
    silentOnStartup: editable.launchOnStartup ? editable.silentOnStartup : false,
    historyLimit: editable.historyLimit,
    pickerRecordLimit: editable.pickerRecordLimit,
    pickerPositionMode: editable.pickerPositionMode,
    restoreClipboardAfterPaste: editable.restoreClipboardAfterPaste,
    pauseMonitoring: editable.pauseMonitoring,
    themeMode: editable.themeMode,
    excludedApps: editable.excludedAppsText
      .split(/\r?\n/)
      .map((value) => value.trim())
      .filter(Boolean),
    searchShortcut: editable.searchShortcut,
    searchShortcutEnabled: editable.searchShortcutEnabled,
    customThemeColors: sanitizeCustomThemeColors(editable.customThemeColors),
  };
}

function isSameSettings(left: UserSetting, right: UserSetting) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function SettingCard({
  action,
  children,
  description,
  title,
}: {
  action?: ReactNode;
  children: ReactNode;
  description?: string;
  title: string;
}) {
  return (
    <div className={CARD_CLASS}>
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-pg-fg-default">{title}</h3>
          {description ? (
            <p className="mt-1 text-sm leading-relaxed text-pg-fg-muted">{description}</p>
          ) : null}
        </div>
        {action ? <div className="shrink-0">{action}</div> : null}
      </div>
      <div className="mt-4 space-y-4">{children}</div>
    </div>
  );
}

function ToggleRow({
  checked,
  description,
  disabled = false,
  id,
  nested = false,
  onChange,
  title,
}: {
  checked: boolean;
  description?: string;
  disabled?: boolean;
  id: string;
  nested?: boolean;
  onChange: (checked: boolean) => void;
  title: string;
}) {
  return (
    <label
      className={`flex items-start gap-3 rounded-xl border px-4 py-3 transition-colors ${
        disabled
          ? "cursor-not-allowed border-pg-border-subtle bg-pg-canvas-default/70"
          : "cursor-pointer border-pg-border-default bg-pg-canvas-default hover:border-pg-border-default"
      } ${nested ? "ml-4" : ""}`}
      htmlFor={id}
    >
      <input
        checked={checked}
        className="mt-0.5 h-4 w-4 rounded border-pg-border-default accent-pg-accent-fg"
        disabled={disabled}
        id={id}
        onChange={(event) => onChange(event.target.checked)}
        type="checkbox"
      />
      <span className="min-w-0">
        <span className={`block text-sm font-medium ${disabled ? "text-pg-fg-subtle" : "text-pg-fg-default"}`}>
          {title}
        </span>
        {description ? (
          <span className="mt-1 block text-xs leading-relaxed text-pg-fg-subtle">
            {description}
          </span>
        ) : null}
      </span>
    </label>
  );
}

function OptionCard({
  checked,
  description,
  label,
  name,
  onSelect,
}: {
  checked: boolean;
  description: string;
  label: string;
  name: string;
  onSelect: () => void;
}) {
  return (
    <label
      className={`flex cursor-pointer items-start gap-3 rounded-xl border px-4 py-3 transition-colors ${
        checked
          ? "border-pg-accent-fg bg-pg-accent-subtle"
          : "border-pg-border-default bg-pg-canvas-default hover:border-pg-border-default"
      }`}
    >
      <input
        checked={checked}
        className="mt-0.5 h-4 w-4 accent-pg-accent-fg"
        name={name}
        onChange={onSelect}
        type="radio"
      />
      <span className="min-w-0">
        <span className={`block text-sm font-medium ${checked ? "text-pg-fg-default" : "text-pg-fg-muted"}`}>
          {label}
        </span>
        <span className="mt-1 block text-xs leading-relaxed text-pg-fg-subtle">
          {description}
        </span>
      </span>
    </label>
  );
}

function SaveStatusText({ saveStatus }: { saveStatus: "idle" | "saving" | "saved" | "error" }) {
  if (saveStatus === "saving") {
    return <span className="text-xs text-pg-fg-subtle">正在保存</span>;
  }

  if (saveStatus === "saved") {
    return <span className="text-xs text-pg-fg-subtle">已保存</span>;
  }

  if (saveStatus === "error") {
    return <span className="text-xs text-pg-danger-fg">保存失败</span>;
  }

  return null;
}

function ThemeColorInput({
  error,
  hint,
  label,
  onChange,
  value,
}: {
  error?: string;
  hint: string;
  label: string;
  onChange: (value: string) => void;
  value: string;
}) {
  return (
    <label className="block">
      <span className={FORM_LABEL}>{label}</span>
      <div className="flex items-center gap-3">
        <span
          aria-hidden="true"
          className="h-9 w-9 shrink-0 rounded-lg border border-pg-border-default bg-pg-canvas-default"
          style={{ backgroundColor: value }}
        />
        <input
          className={FORM_INPUT}
          onChange={(event) => onChange(event.target.value)}
          placeholder="#RRGGBB"
          value={value}
        />
      </div>
      <p className={error ? "mt-1.5 text-xs leading-relaxed text-pg-danger-fg" : FORM_HINT}>
        {error ?? hint}
      </p>
    </label>
  );
}

export function SettingsShell() {
  const settings = useSettingsQuery();
  const updateSettingsMutation = useUpdateSettingsMutation();
  const {
    layoutMode,
    activeSectionId,
    registerContainer,
    registerSection,
    scrollToSection,
  } = useSettingsNavigation();

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
  const [searchShortcut, setSearchShortcut] = useState("Alt+S");
  const [searchShortcutEnabled, setSearchShortcutEnabled] = useState(true);
  const [customThemeColors, setCustomThemeColors] = useState<CustomThemeColors>(DEFAULT_CUSTOM_THEME_COLORS);
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const isInitializingRef = useRef(true);
  const hasHydratedFromServerRef = useRef(false);
  const latestLocalPayloadRef = useRef<UserSetting | null>(null);
  const latestSaveRequestIdRef = useRef(0);
  const hydrationTimerRef = useRef<number | null>(null);

  const applyServerSettings = (nextSettings: UserSetting) => {
    const nextEditable = toEditableSettings(nextSettings);

    latestLocalPayloadRef.current = nextSettings;
    hasHydratedFromServerRef.current = true;
    isInitializingRef.current = true;

    if (hydrationTimerRef.current !== null) {
      window.clearTimeout(hydrationTimerRef.current);
    }

    setShortcut(nextEditable.shortcut);
    setLaunchOnStartup(nextEditable.launchOnStartup);
    setSilentOnStartup(nextEditable.silentOnStartup);
    setHistoryLimit(nextEditable.historyLimit);
    setPickerRecordLimit(nextEditable.pickerRecordLimit);
    setPickerPositionMode(nextEditable.pickerPositionMode);
    setRestoreClipboardAfterPaste(nextEditable.restoreClipboardAfterPaste);
    setPauseMonitoring(nextEditable.pauseMonitoring);
    setThemeMode(nextEditable.themeMode);
    setExcludedAppsText(nextEditable.excludedAppsText);
    setSearchShortcut(nextEditable.searchShortcut);
    setSearchShortcutEnabled(nextEditable.searchShortcutEnabled);
    setCustomThemeColors(nextEditable.customThemeColors);

    hydrationTimerRef.current = window.setTimeout(() => {
      isInitializingRef.current = false;
      hydrationTimerRef.current = null;
    }, 100);
  };

  useEffect(() => {
    latestLocalPayloadRef.current = toSettingsPayload({
      shortcut,
      launchOnStartup,
      silentOnStartup,
      historyLimit,
      pickerRecordLimit,
      pickerPositionMode,
      restoreClipboardAfterPaste,
      pauseMonitoring,
      themeMode,
      excludedAppsText,
      searchShortcut,
      searchShortcutEnabled,
      customThemeColors,
    });
  }, [
    shortcut,
    launchOnStartup,
    silentOnStartup,
    historyLimit,
    pickerRecordLimit,
    pickerPositionMode,
    restoreClipboardAfterPaste,
    pauseMonitoring,
    themeMode,
    excludedAppsText,
    searchShortcut,
    searchShortcutEnabled,
    customThemeColors,
  ]);

  const colorErrors = getCustomThemeColorErrors(customThemeColors);
  const hasColorErrors = Object.keys(colorErrors).length > 0;

  useEffect(() => {
    if (!data) return;
    const currentLocalPayload = latestLocalPayloadRef.current;
    if (
      hasHydratedFromServerRef.current &&
      currentLocalPayload &&
      !isSameSettings(currentLocalPayload, data)
    ) {
      return;
    }

    applyServerSettings(data);
  }, [data]);

  useEffect(() => {
    return () => {
      if (hydrationTimerRef.current !== null) {
        window.clearTimeout(hydrationTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!data) return;
    if (isInitializingRef.current) return;
    if (hasColorErrors) return;
    const payload = latestLocalPayloadRef.current;
    if (!payload) return;
    if (isSameSettings(payload, data)) return;

    const timer = setTimeout(() => {
      const requestId = latestSaveRequestIdRef.current + 1;
      latestSaveRequestIdRef.current = requestId;
      setSaveStatus("saving");
      updateSettingsMutation.mutate(payload, {
        onSuccess: (nextValue, variables) => {
          queryClient.setQueryData(["settings"], nextValue);

          if (requestId !== latestSaveRequestIdRef.current) {
            return;
          }

          if (latestLocalPayloadRef.current && isSameSettings(latestLocalPayloadRef.current, variables)) {
            applyServerSettings(nextValue);
            setSaveStatus("saved");
          }
        },
        onError: () => {
          if (requestId === latestSaveRequestIdRef.current) {
            setSaveStatus("error");
          }
        },
      });
    }, 800);

    return () => clearTimeout(timer);
  }, [
    shortcut,
    launchOnStartup,
    silentOnStartup,
    historyLimit,
    pickerRecordLimit,
    pickerPositionMode,
    restoreClipboardAfterPaste,
    pauseMonitoring,
    themeMode,
    excludedAppsText,
    searchShortcut,
    searchShortcutEnabled,
    customThemeColors,
    hasColorErrors,
    data,
    updateSettingsMutation,
  ]);

  useEffect(() => {
    if (!isTauriRuntime()) return;

    let offSettings: (() => void) | undefined;
    let offOpenSettings: (() => void) | undefined;

    void listen(SETTINGS_CHANGED_EVENT, async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    }).then((cleanup) => {
      offSettings = cleanup;
    });

    void listen(SETTINGS_OPEN_SETTINGS_EVENT, async () => {
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

  const loadError = settings.isError && !data
    ? getErrorMessage(settings.error, "设置加载失败，请稍后重试。")
    : null;
  const saveError = updateSettingsMutation.error
    ? getErrorMessage(updateSettingsMutation.error, "保存设置失败，请稍后重试。")
    : null;
  const updateThemeColor = (
    themeKey: keyof CustomThemeColors,
    field: keyof ThemeColorPalette,
    value: string,
  ) => {
    setCustomThemeColors((current) => ({
      ...current,
      [themeKey]: {
        ...current[themeKey],
        [field]: value,
      },
    }));
  };

  return (
    <main className="flex min-h-screen flex-col bg-pg-canvas-default">
      <div className="mx-auto w-full max-w-[1080px] px-6 py-8" ref={registerContainer}>
        <header className="mb-8 flex flex-col gap-4 border-b border-pg-border-muted pb-6 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <p className="text-[11px] font-semibold uppercase tracking-[0.16em] text-pg-fg-subtle">
              FloatPaste
            </p>
            <h1 className="mt-2 text-2xl font-semibold text-pg-fg-default">设置</h1>
            <p className="mt-2 text-sm text-pg-fg-muted">偏好设置会自动保存。</p>
          </div>
          <div className="flex items-center">
            <SaveStatusText saveStatus={saveStatus} />
          </div>
        </header>

        {saveError ? (
          <div className="mb-6 flex items-start justify-between gap-3 rounded-xl border border-pg-danger-fg/40 bg-pg-danger-subtle px-4 py-3 text-sm text-pg-danger-fg">
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

        {settings.isLoading && !data ? (
          <div className="flex min-h-[320px] items-center justify-center rounded-2xl border border-pg-border-muted bg-pg-canvas-subtle">
            <LoadingSpinner size="sm" text="正在加载设置..." />
          </div>
        ) : loadError ? (
          <div className="rounded-2xl border border-pg-danger-fg/40 bg-pg-danger-subtle px-5 py-5">
            <h2 className="text-sm font-semibold text-pg-danger-fg">设置加载失败</h2>
            <p className="mt-2 text-sm leading-relaxed text-pg-fg-muted">{loadError}</p>
            <button
              className="mt-4 rounded-lg border border-pg-danger-fg/40 px-3 py-2 text-sm font-medium text-pg-danger-fg transition-opacity hover:opacity-80"
              onClick={() => {
                void settings.refetch();
              }}
              type="button"
            >
              重新加载
            </button>
          </div>
        ) : (
          <div className={layoutMode === "sidebar" ? "grid grid-cols-[240px_minmax(0,1fr)] gap-8" : ""}>
            {layoutMode === "sidebar" ? (
              <SettingsNav
                activeSectionId={activeSectionId}
                layoutMode="sidebar"
                onSelect={scrollToSection}
              />
            ) : null}

            <div className="min-w-0">
              {layoutMode === "compact" ? (
                <SettingsNav
                  activeSectionId={activeSectionId}
                  layoutMode="compact"
                  onSelect={scrollToSection}
                />
              ) : null}

              <div className="space-y-10">
                <SettingsSection
                  description={sectionDescriptions.shortcuts}
                  id="shortcuts"
                  registerSection={registerSection}
                  title="快捷键"
                >
                  <SettingCard
                    description="控制速贴面板的全局唤起方式。"
                    title="速贴唤起"
                  >
                    <label className="block">
                      <span className={FORM_LABEL}>全局快捷键</span>
                      <input
                        className={FORM_INPUT}
                        onChange={(event) => setShortcut(event.target.value)}
                        value={shortcut}
                      />
                      <p className={FORM_HINT}>在任意位置快速唤起 FloatPaste 速贴面板。</p>
                    </label>
                  </SettingCard>

                  <SettingCard
                    action={(
                      <label className="flex cursor-pointer items-center gap-2" htmlFor="search-shortcut-enabled">
                        <input
                          checked={searchShortcutEnabled}
                          className="h-4 w-4 rounded border-pg-border-default accent-pg-accent-fg"
                          id="search-shortcut-enabled"
                          onChange={(event) => setSearchShortcutEnabled(event.target.checked)}
                          type="checkbox"
                        />
                        <span className="text-xs text-pg-fg-subtle">启用</span>
                      </label>
                    )}
                    description="为搜索窗口单独保留一组更适合检索场景的快捷键。"
                    title="搜索窗口"
                  >
                    <label className="block">
                      <span className={FORM_LABEL}>搜索窗口快捷键</span>
                      <input
                        className={FORM_INPUT}
                        disabled={!searchShortcutEnabled}
                        onChange={(event) => setSearchShortcut(event.target.value)}
                        placeholder="Alt+S"
                        value={searchShortcut}
                      />
                      <p className={FORM_HINT}>关闭启用开关后会保留当前快捷键值，但暂时不响应。</p>
                    </label>
                  </SettingCard>
                </SettingsSection>

                <SettingsSection
                  description={sectionDescriptions.general}
                  id="general"
                  registerSection={registerSection}
                  title="通用"
                >
                  <SettingCard
                    description="控制历史记录保留规模与速贴面板的一次性浏览密度。"
                    title="历史与列表"
                  >
                    <div className="grid gap-4 md:grid-cols-2">
                      <label className="block">
                        <span className={FORM_LABEL}>历史记录上限</span>
                        <input
                          className={FORM_INPUT}
                          min={100}
                          onChange={(event) => setHistoryLimit(Number(event.target.value) || 1000)}
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
                          onChange={(event) => setPickerRecordLimit(Number(event.target.value) || 50)}
                          type="number"
                          value={pickerRecordLimit}
                        />
                        <p className={FORM_HINT}>
                          控制速贴面板一次可滚动浏览的记录数，数字快捷键仍只覆盖前 9 条。
                        </p>
                      </label>
                    </div>
                  </SettingCard>
                </SettingsSection>

                <SettingsSection
                  description={sectionDescriptions.appearance}
                  id="appearance"
                  registerSection={registerSection}
                  title="外观"
                >
                  <SettingCard
                    description="选择日常使用的界面主题。"
                    title="界面主题"
                  >
                    <div className="space-y-2">
                      {themeModeOptions.map((option) => (
                        <OptionCard
                          checked={themeMode === option.value}
                          description={option.description}
                          key={option.value}
                          label={option.label}
                          name="theme-mode"
                          onSelect={() => setThemeMode(option.value)}
                        />
                      ))}
                    </div>
                  </SettingCard>

                  <SettingCard
                    action={(
                      <button
                        className="rounded-lg border border-pg-border-default px-3 py-2 text-xs font-medium text-pg-fg-muted transition-colors hover:bg-pg-canvas-default hover:text-pg-fg-default"
                        onClick={() => setCustomThemeColors(DEFAULT_CUSTOM_THEME_COLORS)}
                        type="button"
                      >
                        恢复默认
                      </button>
                    )}
                    description="分别为浅色与深色主题输入窗口背景、卡片背景与强调色，Tooltip 会自动同步。"
                    title="自定义颜色"
                  >
                    <div className="grid gap-6 lg:grid-cols-2">
                      {(["light", "dark"] as const).map((themeKey) => (
                        <div
                          className="rounded-xl border border-pg-border-default bg-pg-canvas-default px-4 py-4"
                          key={themeKey}
                        >
                          <div className="mb-4">
                            <h4 className="text-sm font-semibold text-pg-fg-default">
                              {themeKey === "light" ? "浅色主题" : "深色主题"}
                            </h4>
                            <p className="mt-1 text-xs leading-relaxed text-pg-fg-subtle">
                              输入 `#RRGGBB`，例如 `#EFF2F5`。
                            </p>
                          </div>
                          <div className="space-y-4">
                            <ThemeColorInput
                              error={colorErrors[`${themeKey}.windowBg`]}
                              hint="控制窗口主体背景。"
                              label="窗口背景色"
                              onChange={(value) => updateThemeColor(themeKey, "windowBg", value)}
                              value={customThemeColors[themeKey].windowBg}
                            />
                            <ThemeColorInput
                              error={colorErrors[`${themeKey}.cardBg`]}
                              hint="用于卡片、列表项和 tooltip 主体。"
                              label="卡片背景色"
                              onChange={(value) => updateThemeColor(themeKey, "cardBg", value)}
                              value={customThemeColors[themeKey].cardBg}
                            />
                            <ThemeColorInput
                              error={colorErrors[`${themeKey}.accent`]}
                              hint="用于选中态、按钮、焦点与高亮。"
                              label="强调色"
                              onChange={(value) => updateThemeColor(themeKey, "accent", value)}
                              value={customThemeColors[themeKey].accent}
                            />
                          </div>
                        </div>
                      ))}
                    </div>
                  </SettingCard>

                  <SettingCard
                    description="决定速贴窗口在唤起时更贴近哪里的上下文。"
                    title="速贴窗口显示位置"
                  >
                    <div className="space-y-2">
                      {pickerPositionOptions.map((option) => (
                        <OptionCard
                          checked={pickerPositionMode === option.value}
                          description={option.description}
                          key={option.value}
                          label={option.label}
                          name="picker-position-mode"
                          onSelect={() => setPickerPositionMode(option.value)}
                        />
                      ))}
                    </div>
                  </SettingCard>
                </SettingsSection>

                <SettingsSection
                  description={sectionDescriptions.behavior}
                  id="behavior"
                  registerSection={registerSection}
                  title="行为"
                >
                  <SettingCard
                    description="先决定是否跟随系统开机，再配置静默启动这一从属选项。"
                    title="开机启动"
                  >
                    <ToggleRow
                      checked={launchOnStartup}
                      description="登录系统后自动启动 FloatPaste。"
                      id="launch-on-startup"
                      onChange={(checked) => {
                        setLaunchOnStartup(checked);
                        if (!checked) {
                          setSilentOnStartup(false);
                        }
                      }}
                      title="开机自启"
                    />
                    <ToggleRow
                      checked={silentOnStartup}
                      description="仅在已启用开机自启时可用，启动后不主动打断当前工作流。"
                      disabled={!launchOnStartup}
                      id="silent-on-startup"
                      nested
                      onChange={setSilentOnStartup}
                      title="开机时静默启动"
                    />
                  </SettingCard>

                  <SettingCard
                    description="控制贴回完成后的剪贴板处理与监听行为。"
                    title="贴回与监听"
                  >
                    <ToggleRow
                      checked={restoreClipboardAfterPaste}
                      description="贴回完成后恢复原有剪贴板内容，减少对当前工作流的干扰。"
                      id="restore-clipboard"
                      onChange={setRestoreClipboardAfterPaste}
                      title="回贴后恢复剪贴板"
                    />
                    <ToggleRow
                      checked={pauseMonitoring}
                      description="暂停后不会继续采集新的剪贴板记录。"
                      id="pause-monitoring"
                      onChange={setPauseMonitoring}
                      title="暂停监听"
                    />
                  </SettingCard>
                </SettingsSection>

                <SettingsSection
                  description={sectionDescriptions.excludedApps}
                  id="excludedApps"
                  registerSection={registerSection}
                  title="排除应用"
                >
                  <SettingCard
                    description="每行填写一个可执行文件名，命中的应用不会被采集进历史记录。"
                    title="忽略指定进程"
                  >
                    <label className="block">
                      <span className={FORM_LABEL}>进程列表</span>
                      <textarea
                        className={`${FORM_INPUT} min-h-[140px] leading-relaxed`}
                        onChange={(event) => setExcludedAppsText(event.target.value)}
                        placeholder={"每行一个可执行文件名，例如：\nKeePass.exe\nWindowsTerminal.exe"}
                        value={excludedAppsText}
                      />
                      <p className={FORM_HINT}>建议使用完整进程名，避免误伤其他应用。</p>
                    </label>
                  </SettingCard>
                </SettingsSection>
              </div>
            </div>
          </div>
        )}
      </div>
    </main>
  );
}
