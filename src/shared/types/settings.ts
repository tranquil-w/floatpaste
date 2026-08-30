import type { ThemePresetId } from "../theme/types.ts";

export type PickerPositionMode = "mouse" | "lastPosition" | "caret";
export type ThemeMode = "system" | "light" | "dark";

export interface UserSetting {
  shortcut: string;
  launchOnStartup: boolean;
  silentOnStartup: boolean;
  historyLimit: number;
  pickerRecordLimit: number;
  pickerPositionMode: PickerPositionMode;
  excludedApps: string[];
  restoreClipboardAfterPaste: boolean;
  pauseMonitoring: boolean;
  themeMode: ThemeMode;
  /** 主题预设 id，见 shared/theme/presets.ts */
  themePreset: ThemePresetId;
  /**
   * 强调色取值："default"=跟随预设 | 安全列表 id | 旧版迁移保留的 #RRGGBB。
   * 非法值在派生时一律回退预设自带强调色。
   */
  themeAccent: string;
  searchShortcut: string;
  searchShortcutEnabled: boolean;
  /** Picker 会话期数字键 1-9 直达；无修饰全局热键冲突面大，可关闭 */
  pickerDigitShortcutsEnabled: boolean;
}
