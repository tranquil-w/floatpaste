export type PickerPositionMode = "mouse" | "lastPosition" | "caret";
export type ThemeMode = "system" | "light" | "dark";
export type ThemeColorPalette = {
  windowBg: string;
  cardBg: string;
  accent: string;
};

export type CustomThemeColors = {
  light: ThemeColorPalette;
  dark: ThemeColorPalette;
};

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
  searchShortcut: string;
  searchShortcutEnabled: boolean;
  /** Picker 会话期数字键 1-9 直达；无修饰全局热键冲突面大，可关闭 */
  pickerDigitShortcutsEnabled: boolean;
  customThemeColors: CustomThemeColors;
}
