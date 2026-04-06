export type SettingsSectionId =
  | "shortcuts"
  | "general"
  | "appearance"
  | "behavior"
  | "excludedApps";

export type SettingsSectionMeta = {
  id: SettingsSectionId;
  label: string;
  description: string;
};

export const SETTINGS_SECTIONS: SettingsSectionMeta[] = [
  { id: "shortcuts", label: "快捷键", description: "全局唤起与搜索" },
  { id: "general", label: "通用", description: "历史上限与列表容量" },
  { id: "appearance", label: "外观", description: "主题与窗口位置" },
  { id: "behavior", label: "行为", description: "启动与监听策略" },
  { id: "excludedApps", label: "排除应用", description: "忽略指定进程" },
];
