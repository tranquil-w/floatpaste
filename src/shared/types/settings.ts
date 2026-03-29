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
  searchShortcut: string;
  searchShortcutEnabled: boolean;
}
