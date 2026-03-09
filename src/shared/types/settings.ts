export interface UserSetting {
  shortcut: string;
  launchOnStartup: boolean;
  silentOnStartup: boolean;
  historyLimit: number;
  excludedApps: string[];
  restoreClipboardAfterPaste: boolean;
  pauseMonitoring: boolean;
}
