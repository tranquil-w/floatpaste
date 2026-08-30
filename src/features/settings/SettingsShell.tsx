import { useEffect, useRef, useState, type ReactNode } from "react";
import { SETTINGS_CHANGED_EVENT, SETTINGS_OPEN_SETTINGS_EVENT } from "../../bridge/events";
import { isTauriRuntime } from "../../bridge/runtime";
import { hideCurrentWindow } from "../../bridge/window";
import { queryClient } from "../../app/queryClient";
import { useAppEvent } from "../../shared/hooks/useAppEvent";
import type { PickerPositionMode, ThemeMode, UserSetting } from "../../shared/types/settings";
import { DEFAULT_THEME_ACCENT, DEFAULT_THEME_PRESET } from "../../shared/theme";
import type { ResolvedTheme } from "../../shared/theme";
import { LoadingSpinner } from "../../shared/ui/LoadingSpinner";
import { getErrorMessage } from "../../shared/utils/error";
import { SettingsNav } from "./SettingsNav";
import { SettingsSection } from "./SettingsSection";
import { ShortcutInput } from "./ShortcutInput";
import { type SettingsSectionId } from "./settingsSections";
import { ThemeAccentPicker, ThemePresetPicker } from "./ThemePresetPicker";
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
  themePreset: UserSetting["themePreset"];
  themeAccent: string;
  excludedAppsText: string;
  searchShortcut: string;
  searchShortcutEnabled: boolean;
  pickerDigitShortcutsEnabled: boolean;
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
    description: "固定使用浅色界面，不受系统外观影响。",
  },
  {
    value: "dark",
    label: "深色",
    description: "固定使用深色界面，适合夜间与暗光环境。",
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
  "rounded-xl border border-pg-border-default bg-pg-canvas-default px-3 py-1.5 text-sm outline-none transition-colors placeholder:text-pg-fg-subtle focus:border-pg-accent-fg focus:ring-1 focus:ring-pg-accent-fg focus-visible:outline-none disabled:cursor-not-allowed disabled:border-pg-border-subtle disabled:bg-pg-canvas-subtle disabled:text-pg-fg-subtle";

/** 分组行容器：一个 section 内的设置项拍平为带分隔线的行列表 */
const ROW_GROUP_CLASS =
  "divide-y divide-pg-border-subtle overflow-hidden rounded-xl border border-pg-border-muted bg-pg-canvas-subtle";

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
    themePreset: settings.themePreset,
    themeAccent: settings.themeAccent,
    excludedAppsText: settings.excludedApps.join("\n"),
    searchShortcut: settings.searchShortcut,
    searchShortcutEnabled: settings.searchShortcutEnabled,
    pickerDigitShortcutsEnabled: settings.pickerDigitShortcutsEnabled,
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
    themePreset: editable.themePreset,
    themeAccent: editable.themeAccent,
    excludedApps: editable.excludedAppsText
      .split(/\r?\n/)
      .map((value) => value.trim())
      .filter(Boolean),
    searchShortcut: editable.searchShortcut,
    searchShortcutEnabled: editable.searchShortcutEnabled,
    pickerDigitShortcutsEnabled: editable.pickerDigitShortcutsEnabled,
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

/**
 * 设置行：左侧标题 + 描述，右侧控件；wide 变体用于需要通栏的控件
 * （文本域、预设网格等），多个行配合 ROW_GROUP_CLASS 组成一个分组。
 */
function SettingRow({
  children,
  description,
  title,
  wide = false,
}: {
  children: ReactNode;
  description?: string;
  title: string;
  wide?: boolean;
}) {
  if (wide) {
    return (
      <div className="px-4 py-3">
        <h3 className="text-sm font-medium text-pg-fg-default">{title}</h3>
        {description ? (
          <p className="mt-0.5 text-xs leading-relaxed text-pg-fg-muted">{description}</p>
        ) : null}
        <div className="mt-2.5">{children}</div>
      </div>
    );
  }

  return (
    <div className="flex items-center gap-6 px-4 py-2.5">
      <div className="min-w-0 flex-1">
        <h3 className="text-sm font-medium text-pg-fg-default">{title}</h3>
        {description ? (
          <p className="mt-0.5 text-xs leading-relaxed text-pg-fg-muted">{description}</p>
        ) : null}
      </div>
      {children ? <div className="flex shrink-0 items-center gap-4">{children}</div> : null}
    </div>
  );
}

/** 分段单选控件：取代纵向 radio 列表，选中项的说明由调用方作为行描述展示 */
function SegmentedControl<T extends string>({
  name,
  onChange,
  options,
  value,
}: {
  name: string;
  onChange: (value: T) => void;
  options: Array<{ label: string; value: T }>;
  value: T;
}) {
  return (
    <div
      aria-label={name}
      className="flex rounded-lg border border-pg-border-default bg-pg-canvas-default p-0.5"
      role="radiogroup"
    >
      {options.map((option) => {
        const active = option.value === value;
        return (
          <button
            aria-checked={active}
            className={`rounded-md px-2.5 py-1 text-[13px] transition-colors ${
              active
                ? "bg-pg-accent-subtle font-medium text-pg-fg-default"
                : "text-pg-fg-muted hover:text-pg-fg-default"
            }`}
            key={option.value}
            onClick={() => onChange(option.value)}
            role="radio"
            type="button"
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

/** 开关行：标题与描述在左、复选框在右的拍平行 */
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
      className={`flex items-center gap-6 px-4 py-2 transition-colors ${
        disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer hover:bg-pg-canvas-default"
      } ${nested ? "pl-9" : ""}`}
      htmlFor={id}
    >
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-medium text-pg-fg-default">{title}</span>
        {description ? (
          <span className="mt-0.5 block text-xs leading-relaxed text-pg-fg-subtle">
            {description}
          </span>
        ) : null}
      </span>
      <input
        checked={checked}
        className="h-4 w-4 shrink-0 rounded accent-pg-accent-fg"
        disabled={disabled}
        id={id}
        onChange={(event) => onChange(event.target.checked)}
        type="checkbox"
      />
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

export function SettingsShell({ resolvedTheme }: { resolvedTheme: ResolvedTheme }) {
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
  const [themePreset, setThemePreset] = useState<UserSetting["themePreset"]>(DEFAULT_THEME_PRESET);
  const [themeAccent, setThemeAccent] = useState(DEFAULT_THEME_ACCENT);
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const isInitializingRef = useRef(true);
  const hasHydratedFromServerRef = useRef(false);
  const latestLocalPayloadRef = useRef<UserSetting | null>(null);
  const latestSaveRequestIdRef = useRef(0);
  const hydrationTimerRef = useRef<number | null>(null);
  // 上一份服务端设置：用于区分"数据变化来自外部（托盘等）"与"本地未保存编辑导致的差异"。
  // 外部变更必须强制水合，否则防抖自动保存会把本地旧值写回、覆盖外部修改。
  const lastServerSettingsRef = useRef<UserSetting | null>(null);
  // 自己最近一次保存成功写入的值：保存回显不算外部变更，
  // 否则强制水合会回滚保存在途期间的新编辑，且保存状态卡在"正在保存"。
  const lastOwnWriteRef = useRef<UserSetting | null>(null);
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
    setThemePreset(nextEditable.themePreset);
    setThemeAccent(nextEditable.themeAccent);
    setExcludedAppsText(nextEditable.excludedAppsText);
    setSearchShortcut(nextEditable.searchShortcut);
    setSearchShortcutEnabled(nextEditable.searchShortcutEnabled);
    setPickerDigitShortcutsEnabled(nextEditable.pickerDigitShortcutsEnabled);

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
      themePreset,
      themeAccent,
      excludedAppsText,
      searchShortcut,
      searchShortcutEnabled,
      pickerDigitShortcutsEnabled,
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
    themePreset,
    themeAccent,
    excludedAppsText,
    searchShortcut,
    searchShortcutEnabled,
    pickerDigitShortcutsEnabled,
  ]);

  // 两处全局快捷键冲突时后端会强制重置搜索键，前端提前拦截保存并提示
  const shortcutConflict =
    searchShortcutEnabled &&
    shortcut.trim() !== "" &&
    normalizeShortcutValue(shortcut) === normalizeShortcutValue(searchShortcut);
  // 存在非法输入时自动保存被拦截，头部需明示"有修改未落盘"，避免用户误以为已保存
  const saveBlockedReasons = [shortcutConflict ? "快捷键冲突" : null].filter(
    (reason): reason is string => reason !== null,
  );

  // 立即保存指定载荷；供 debounce 回调、失败重试与关窗前 flush 共用
  const performSave = (payload: UserSetting) =>
    new Promise<void>((resolve) => {
      const requestId = latestSaveRequestIdRef.current + 1;
      latestSaveRequestIdRef.current = requestId;
      setSaveStatus("saving");
      updateSettingsMutation.mutate(payload, {
        onSuccess: (nextValue, variables) => {
          queryClient.setQueryData(settingsQueryKey, nextValue);
          lastOwnWriteRef.current = nextValue;

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
    const previousServer = lastServerSettingsRef.current;
    lastServerSettingsRef.current = data;
    const isOwnWriteEcho =
      lastOwnWriteRef.current !== null && isSameSettings(lastOwnWriteRef.current, data);
    const isExternalChange =
      previousServer !== null && !isSameSettings(previousServer, data) && !isOwnWriteEcho;
    if (
      !isExternalChange &&
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
    if (shortcutConflict) return;
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
    themePreset,
    themeAccent,
    excludedAppsText,
    searchShortcut,
    searchShortcutEnabled,
    pickerDigitShortcutsEnabled,
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

  return (
    <main className="flex min-h-screen flex-col bg-pg-canvas-default">
      <div className="mx-auto w-full max-w-[1080px] px-5 py-3.5" ref={registerContainer}>
        <header className="sticky top-0 z-20 -mx-5 mb-3.5 flex items-center justify-between border-b border-pg-border-muted bg-pg-canvas-default px-5 py-2">
          <h1 className="text-base font-semibold text-pg-fg-default">设置</h1>
          <SaveStatusText saveBlockedReasons={saveBlockedReasons} saveStatus={saveStatus} />
        </header>

        {saveError ? (
          <div className="mb-3.5 flex items-start justify-between gap-3 rounded-xl border border-pg-danger-fg/40 bg-pg-danger-subtle px-4 py-2.5 text-sm text-pg-danger-fg">
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
            className={layoutMode === "sidebar" ? "grid grid-cols-[176px_minmax(0,1fr)] gap-5" : ""}
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

              <div className="space-y-4">
                <SettingsSection
                  description={sectionDescriptions.general}
                  id="general"
                  registerSection={registerSection}
                  title="通用"
                >
                  <div className={ROW_GROUP_CLASS}>
                    <SettingRow description="超出后自动清理最早的记录。" title="历史记录上限">
                      <input
                        className={`${FORM_INPUT} w-24 text-right`}
                        max={10000}
                        min={100}
                        onChange={(event) =>
                          setHistoryLimit(toBoundedNumber(event.target.value, 100, 10000, 1000))
                        }
                        step={100}
                        type="number"
                        value={historyLimit}
                      />
                    </SettingRow>
                    <SettingRow
                      description="速贴面板一次可滚动浏览的记录数，数字快捷键只覆盖前 9 条。"
                      title="速贴窗口记录数"
                    >
                      <input
                        className={`${FORM_INPUT} w-24 text-right`}
                        max={1000}
                        min={9}
                        onChange={(event) =>
                          setPickerRecordLimit(toBoundedNumber(event.target.value, 9, 1000, 50))
                        }
                        type="number"
                        value={pickerRecordLimit}
                      />
                    </SettingRow>
                  </div>
                </SettingsSection>

                <SettingsSection
                  description={sectionDescriptions.shortcuts}
                  id="shortcuts"
                  registerSection={registerSection}
                  title="快捷键"
                >
                  <div className={ROW_GROUP_CLASS}>
                    <SettingRow description="全局唤出速贴面板的组合键。" title="速贴唤起">
                      <div className="w-56">
                        <ShortcutInput onChange={setShortcut} value={shortcut} />
                      </div>
                    </SettingRow>
                    <SettingRow
                      description="为搜索窗口单独保留一组更适合检索场景的快捷键。"
                      title="搜索窗口"
                    >
                      <label
                        className="flex cursor-pointer items-center gap-1.5"
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
                      <div className="w-56">
                        <ShortcutInput
                          disabled={!searchShortcutEnabled}
                          hint={
                            shortcutConflict
                              ? "与主快捷键相同，请换一组组合；冲突时不会保存。"
                              : undefined
                          }
                          onChange={setSearchShortcut}
                          value={searchShortcut}
                        />
                      </div>
                    </SettingRow>
                  </div>
                </SettingsSection>

                <SettingsSection
                  description={sectionDescriptions.appearance}
                  id="appearance"
                  registerSection={registerSection}
                  title="外观"
                >
                  <div className={ROW_GROUP_CLASS}>
                    <SettingRow
                      description={
                        themeModeOptions.find((option) => option.value === themeMode)?.description
                      }
                      title="界面模式"
                    >
                      <SegmentedControl
                        name="界面模式"
                        onChange={setThemeMode}
                        options={themeModeOptions}
                        value={themeMode}
                      />
                    </SettingRow>

                    <SettingRow
                      description="配色方案基于经过验证的公开色板，全部通过对比度门禁，跨设备观感一致。"
                      title="主题预设"
                      wide
                    >
                      <ThemePresetPicker
                        onSelectPreset={setThemePreset}
                        resolvedTheme={resolvedTheme}
                        themeAccent={themeAccent}
                        themePreset={themePreset}
                      />
                    </SettingRow>

                    <SettingRow description="选中态、按钮与高亮使用的颜色。" title="强调色">
                      <ThemeAccentPicker
                        onSelectAccent={setThemeAccent}
                        resolvedTheme={resolvedTheme}
                        themeAccent={themeAccent}
                        themePreset={themePreset}
                      />
                    </SettingRow>

                    <SettingRow
                      description={
                        pickerPositionOptions.find((option) => option.value === pickerPositionMode)
                          ?.description
                      }
                      title="速贴窗口显示位置"
                    >
                      <SegmentedControl
                        name="速贴窗口显示位置"
                        onChange={setPickerPositionMode}
                        options={pickerPositionOptions}
                        value={pickerPositionMode}
                      />
                    </SettingRow>
                  </div>
                </SettingsSection>

                <SettingsSection
                  description={sectionDescriptions.behavior}
                  id="behavior"
                  registerSection={registerSection}
                  title="行为"
                >
                  <div className={ROW_GROUP_CLASS}>
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
                      description="启动后不主动打断当前工作流。"
                      disabled={!launchOnStartup}
                      id="silent-on-startup"
                      nested
                      onChange={setSilentOnStartup}
                      title="开机时静默启动"
                    />
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
                  </div>
                </SettingsSection>

                <SettingsSection
                  description={sectionDescriptions.excludedApps}
                  id="excludedApps"
                  registerSection={registerSection}
                  title="排除应用"
                >
                  <div className={ROW_GROUP_CLASS}>
                    <SettingRow
                      description="每行填写一个可执行文件名，命中的应用不会被采集进历史记录。建议使用完整进程名，避免误伤其他应用。"
                      title="忽略指定进程"
                      wide
                    >
                      <textarea
                        className={`${FORM_INPUT} min-h-[104px] w-full leading-relaxed`}
                        onChange={(event) => setExcludedAppsText(event.target.value)}
                        placeholder={
                          "每行一个可执行文件名，例如：\nKeePass.exe\nWindowsTerminal.exe"
                        }
                        value={excludedAppsText}
                      />
                    </SettingRow>
                  </div>
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
