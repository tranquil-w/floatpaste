import { useEffect, useRef, useState, type ReactNode } from "react";
import { SETTINGS_CHANGED_EVENT, SETTINGS_OPEN_SETTINGS_EVENT } from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import { hideCurrentWindow } from "../../bridge/window";
import { queryClient } from "../../app/queryClient";
import { useAppEvent } from "../../shared/hooks/useAppEvent";
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
import { ShortcutInput } from "./ShortcutInput";
import { type SettingsSectionId } from "./settingsSections";
import { useSettingsNavigation } from "./useSettingsNavigation";
import { TagsSection } from "./TagsSection";
import { useSettingsQuery, useUpdateSettingsMutation } from "./queries";
import { invalidateSettings, settingsQueryKey } from "../../shared/queries/settingsQuery";

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
  pickerDigitShortcutsEnabled: boolean;
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
  tags: "统一管理剪贴记录标签：重命名、合并与删除。",
};

const FORM_INPUT =
  "w-full rounded-xl border border-pg-border-default bg-pg-canvas-default px-4 py-2.5 text-sm outline-none transition-colors placeholder:text-pg-fg-subtle focus:border-pg-accent-fg focus:ring-1 focus:ring-pg-accent-fg focus-visible:outline-none disabled:cursor-not-allowed disabled:border-pg-border-subtle disabled:bg-pg-canvas-subtle disabled:text-pg-fg-subtle";

const FORM_LABEL = "mb-1.5 block text-sm font-medium text-pg-fg-default";
const FORM_HINT = "mt-1.5 text-xs leading-relaxed text-pg-fg-subtle";
const CARD_CLASS =
  "rounded-xl border border-pg-border-muted bg-pg-canvas-subtle px-5 py-5 shadow-sm";

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
    pickerDigitShortcutsEnabled: settings.pickerDigitShortcutsEnabled,
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
    pickerDigitShortcutsEnabled: editable.pickerDigitShortcutsEnabled,
    customThemeColors: sanitizeCustomThemeColors(editable.customThemeColors),
  };
}

function isSameSettings(left: UserSetting, right: UserSetting) {
  return JSON.stringify(left) === JSON.stringify(right);
}

/** 与后端比较口径对齐的快捷键归一化：忽略大小写与修饰键顺序 */
function normalizeShortcutValue(value: string): string {
  return value
    .split("+")
    .map((part) => part.trim().toLowerCase())
    .filter(Boolean)
    .sort()
    .join("+");
}

/** 数字输入钳制：越界或非法值收敛到边界/回退值，与后端约束一致 */
function toBoundedNumber(value: string, min: number, max: number, fallback: number): number {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  return Math.min(max, Math.max(min, Math.round(parsed)));
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
      <div className="mt-3 space-y-1.5">{children}</div>
    </div>
  );
}

/** 设置行：拍平的列表行（无边框盒），悬停才有底色，消除"框中框"嵌套感 */
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
      className={`flex items-start gap-3 rounded-lg px-3 py-2.5 transition-colors ${
        disabled
          ? "cursor-not-allowed opacity-60"
          : "cursor-pointer hover:bg-pg-canvas-default"
      } ${nested ? "ml-6" : ""}`}
      htmlFor={id}
    >
      <input
        checked={checked}
        className="mt-0.5 h-4 w-4 rounded accent-pg-accent-fg"
        disabled={disabled}
        id={id}
        onChange={(event) => onChange(event.target.checked)}
        type="checkbox"
      />
      <span className="min-w-0">
        <span className="block text-sm font-medium text-pg-fg-default">{title}</span>
        {description ? (
          <span className="mt-0.5 block text-xs leading-relaxed text-pg-fg-subtle">
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
      className={`flex cursor-pointer items-start gap-3 rounded-lg px-3 py-2.5 transition-colors ${
        checked ? "bg-pg-accent-subtle" : "hover:bg-pg-canvas-default"
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
        <span
          className={`block text-sm font-medium ${checked ? "text-pg-fg-default" : "text-pg-fg-muted"}`}
        >
          {label}
        </span>
        <span className="mt-0.5 block text-xs leading-relaxed text-pg-fg-subtle">{description}</span>
      </span>
    </label>
  );
}

/** 自动保存状态指示：状态点 + 文案，空闲时提示"自动保存"语义 */
function SaveStatusText({
  saveBlockedReasons,
  saveStatus,
}: {
  saveBlockedReasons: string[];
  saveStatus: "idle" | "saving" | "saved" | "error";
}) {
  if (saveBlockedReasons.length > 0) {
    return (
      <span className="flex items-center gap-1.5 text-xs text-pg-danger-fg" role="alert">
        <span className="h-1.5 w-1.5 rounded-full bg-pg-danger-fg" />
        {`修改暂未保存：${saveBlockedReasons.join("、")}`}
      </span>
    );
  }

  if (saveStatus === "saving") {
    return (
      <span className="flex items-center gap-1.5 text-xs text-pg-fg-subtle">
        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-pg-accent-fg" />
        正在保存
      </span>
    );
  }

  if (saveStatus === "saved") {
    return (
      <span className="flex items-center gap-1.5 text-xs text-pg-fg-subtle">
        <span className="h-1.5 w-1.5 rounded-full bg-pg-success-fg" />
        已保存
      </span>
    );
  }

  if (saveStatus === "error") {
    return (
      <span className="flex items-center gap-1.5 text-xs text-pg-danger-fg">
        <span className="h-1.5 w-1.5 rounded-full bg-pg-danger-fg" />
        保存失败
      </span>
    );
  }

  return <span className="text-xs text-pg-fg-subtle">更改将自动保存</span>;
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
        <input
          aria-label={`${label} 拾色器`}
          className="color-swatch h-9 w-9 shrink-0 cursor-pointer rounded-lg border border-pg-border-default bg-pg-canvas-default p-0.5"
          onChange={(event) => onChange(event.target.value.toUpperCase())}
          type="color"
          value={/^#[0-9a-fA-F]{6}$/.test(value) ? value : "#000000"}
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
  const { layoutMode, activeSectionId, registerContainer, registerSection, scrollToSection } =
    useSettingsNavigation();

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
  const [pickerDigitShortcutsEnabled, setPickerDigitShortcutsEnabled] = useState(true);
  const [customThemeColors, setCustomThemeColors] = useState<CustomThemeColors>(
    DEFAULT_CUSTOM_THEME_COLORS,
  );
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const isInitializingRef = useRef(true);
  const hasHydratedFromServerRef = useRef(false);
  const latestLocalPayloadRef = useRef<UserSetting | null>(null);
  const latestSaveRequestIdRef = useRef(0);
  const hydrationTimerRef = useRef<number | null>(null);
  // Escape 关窗路径经 ref 调用，避免 keydown 监听闭包过期
  const flushPendingSaveRef = useRef<() => Promise<void>>(async () => {});
  const performSaveRef = useRef<(payload: UserSetting) => Promise<void>>(async () => {});

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
    setPickerDigitShortcutsEnabled(nextEditable.pickerDigitShortcutsEnabled);
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
      pickerDigitShortcutsEnabled,
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
    pickerDigitShortcutsEnabled,
    customThemeColors,
  ]);

  const colorErrors = getCustomThemeColorErrors(customThemeColors);
  const hasColorErrors = Object.keys(colorErrors).length > 0;
  // 两处全局快捷键冲突时后端会强制重置搜索键，前端提前拦截保存并提示
  const shortcutConflict =
    searchShortcutEnabled &&
    shortcut.trim() !== "" &&
    normalizeShortcutValue(shortcut) === normalizeShortcutValue(searchShortcut);
  // 存在非法输入时自动保存被拦截，头部需明示"有修改未落盘"，避免用户误以为已保存
  const saveBlockedReasons = [
    hasColorErrors ? "无效的颜色值" : null,
    shortcutConflict ? "快捷键冲突" : null,
  ].filter((reason): reason is string => reason !== null);

  // 立即保存指定载荷；供 debounce 回调、失败重试与关窗前 flush 共用
  const performSave = (payload: UserSetting) =>
    new Promise<void>((resolve) => {
      const requestId = latestSaveRequestIdRef.current + 1;
      latestSaveRequestIdRef.current = requestId;
      setSaveStatus("saving");
      updateSettingsMutation.mutate(payload, {
        onSuccess: (nextValue, variables) => {
          queryClient.setQueryData(settingsQueryKey, nextValue);

          if (requestId !== latestSaveRequestIdRef.current) {
            return;
          }

          if (
            latestLocalPayloadRef.current &&
            isSameSettings(latestLocalPayloadRef.current, variables)
          ) {
            applyServerSettings(nextValue);
            setSaveStatus("saved");
          }
        },
        onError: () => {
          if (requestId === latestSaveRequestIdRef.current) {
            setSaveStatus("error");
          }
        },
        onSettled: () => resolve(),
      });
    });

  // 关窗前把 debounce 中尚未落盘的修改立即保存，避免最后一次改动静默丢失
  async function flushPendingSave() {
    const payload = latestLocalPayloadRef.current;
    if (
      !payload ||
      isInitializingRef.current ||
      hasColorErrors ||
      shortcutConflict ||
      !data ||
      isSameSettings(payload, data)
    ) {
      return;
    }

    await performSave(payload);
  }

  // 保存/flush 走 ref 分发：debounce effect 与 Escape 监听不因每次渲染的新函数引用而重置
  useEffect(() => {
    performSaveRef.current = performSave;
    flushPendingSaveRef.current = flushPendingSave;
  });

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
    if (hasColorErrors || shortcutConflict) return;
    const payload = latestLocalPayloadRef.current;
    if (!payload) return;
    if (isSameSettings(payload, data)) return;

    const timer = setTimeout(() => {
      void performSaveRef.current(payload);
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
    pickerDigitShortcutsEnabled,
    customThemeColors,
    hasColorErrors,
    shortcutConflict,
    data,
  ]);

  useAppEvent(SETTINGS_CHANGED_EVENT, async () => {
    await invalidateSettings(queryClient);
  });

  useAppEvent(SETTINGS_OPEN_SETTINGS_EVENT, async () => {
    await invalidateSettings(queryClient);
  });

  useEffect(() => {
    if (!isTauriRuntime()) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        // 先保存 debounce 中未落盘的修改再关窗，避免最后一次改动静默丢失
        void flushPendingSaveRef
          .current()
          .then(() => hideCurrentWindow())
          .catch(console.error);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  const loadError =
    settings.isError && !data
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
      <div className="mx-auto w-full max-w-[1080px] px-6 py-6" ref={registerContainer}>
        <header className="sticky top-0 z-20 -mx-6 mb-6 flex items-center justify-between border-b border-pg-border-muted bg-pg-canvas-default px-6 py-3.5">
          <h1 className="text-xl font-semibold text-pg-fg-default">设置</h1>
          <SaveStatusText saveBlockedReasons={saveBlockedReasons} saveStatus={saveStatus} />
        </header>

        {saveError ? (
          <div className="mb-6 flex items-start justify-between gap-3 rounded-xl border border-pg-danger-fg/40 bg-pg-danger-subtle px-4 py-3 text-sm text-pg-danger-fg">
            <p>{saveError}</p>
            <div className="flex shrink-0 items-center gap-3">
              <button
                className="text-xs font-semibold uppercase tracking-wider transition-opacity hover:opacity-80"
                onClick={() => {
                  const payload = latestLocalPayloadRef.current;
                  if (payload) {
                    void performSave(payload);
                  }
                }}
                type="button"
              >
                重试
              </button>
              <button
                className="text-xs font-semibold uppercase tracking-wider transition-opacity hover:opacity-80"
                onClick={() => updateSettingsMutation.reset()}
                type="button"
              >
                关闭
              </button>
            </div>
          </div>
        ) : null}

        {settings.isLoading && !data ? (
          <div className="flex min-h-[320px] items-center justify-center rounded-xl border border-pg-border-muted bg-pg-canvas-subtle">
            <LoadingSpinner size="sm" text="正在加载设置..." />
          </div>
        ) : loadError ? (
          <div className="rounded-xl border border-pg-danger-fg/40 bg-pg-danger-subtle px-5 py-5">
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
          <div
            className={layoutMode === "sidebar" ? "grid grid-cols-[200px_minmax(0,1fr)] gap-6" : ""}
          >
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

              <div className="space-y-8">
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
                          max={10000}
                          min={100}
                          onChange={(event) =>
                            setHistoryLimit(toBoundedNumber(event.target.value, 100, 10000, 1000))
                          }
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
                          onChange={(event) =>
                            setPickerRecordLimit(toBoundedNumber(event.target.value, 9, 1000, 50))
                          }
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
                  description={sectionDescriptions.shortcuts}
                  id="shortcuts"
                  registerSection={registerSection}
                  title="快捷键"
                >
                  <SettingCard description="控制速贴面板的全局唤起方式。" title="速贴唤起">
                    <ShortcutInput
                      hint="点击后直接按下组合键录制，例如 Alt+Q。"
                      onChange={setShortcut}
                      value={shortcut}
                    />
                  </SettingCard>

                  <SettingCard
                    action={
                      <label
                        className="flex cursor-pointer items-center gap-2"
                        htmlFor="search-shortcut-enabled"
                      >
                        <input
                          checked={searchShortcutEnabled}
                          className="h-4 w-4 rounded border-pg-border-default accent-pg-accent-fg"
                          id="search-shortcut-enabled"
                          onChange={(event) => setSearchShortcutEnabled(event.target.checked)}
                          type="checkbox"
                        />
                        <span className="text-xs text-pg-fg-subtle">启用</span>
                      </label>
                    }
                    description="为搜索窗口单独保留一组更适合检索场景的快捷键。"
                    title="搜索窗口"
                  >
                    <ShortcutInput
                      disabled={!searchShortcutEnabled}
                      hint={
                        shortcutConflict
                          ? "与主快捷键相同，请换一组组合；冲突时不会保存。"
                          : "关闭启用开关后会保留当前快捷键值，但暂时不响应。"
                      }
                      onChange={setSearchShortcut}
                      value={searchShortcut}
                    />
                  </SettingCard>
                </SettingsSection>

                <SettingsSection
                  description={sectionDescriptions.appearance}
                  id="appearance"
                  registerSection={registerSection}
                  title="外观"
                >
                  <SettingCard description="选择日常使用的界面主题。" title="界面主题">
                    <div className="space-y-0.5">
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
                    action={
                      <button
                        className="rounded-lg border border-pg-border-default px-3 py-2 text-xs font-medium text-pg-fg-muted transition-colors hover:bg-pg-canvas-default hover:text-pg-fg-default"
                        onClick={() => setCustomThemeColors(DEFAULT_CUSTOM_THEME_COLORS)}
                        type="button"
                      >
                        恢复默认
                      </button>
                    }
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
                    <div className="space-y-0.5">
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
                    <ToggleRow
                      checked={pickerDigitShortcutsEnabled}
                      description="速贴面板打开时按 1-9 直接粘贴对应条目。数字键是无修饰全局热键，若与其他软件冲突可关闭。"
                      id="picker-digit-shortcuts"
                      onChange={setPickerDigitShortcutsEnabled}
                      title="速贴面板数字键直达"
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
                        placeholder={
                          "每行一个可执行文件名，例如：\nKeePass.exe\nWindowsTerminal.exe"
                        }
                        value={excludedAppsText}
                      />
                      <p className={FORM_HINT}>建议使用完整进程名，避免误伤其他应用。</p>
                    </label>
                  </SettingCard>
                </SettingsSection>

                <TagsSection registerSection={registerSection} />
              </div>
            </div>
          </div>
        )}
      </div>
    </main>
  );
}
