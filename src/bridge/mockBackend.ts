import type {
  ClipItemDetail,
  ClipItemSummary,
  PasteOption,
  PasteResult,
  SearchQuery,
  SearchResult,
} from "../shared/types/clips";
import type { UserSetting } from "../shared/types/settings";

const now = Date.now();
const pickerPositionModes = new Set(["mouse", "lastPosition", "caret"]);
const themeModes = new Set(["system", "light", "dark"]);
const DEFAULT_MAIN_SHORTCUT = "Alt+Q";
const DEFAULT_SEARCH_SHORTCUT = "Alt+S";
const LEGACY_MAIN_SHORTCUT = "Ctrl+`";
const LEGACY_SEARCH_SHORTCUTS = new Set(["win+f", "windows+f", "super+f"]);

function normalizeMainShortcut(shortcut: string): string {
  const trimmed = shortcut.trim();
  if (!trimmed || trimmed.toLowerCase() === LEGACY_MAIN_SHORTCUT.toLowerCase()) {
    return DEFAULT_MAIN_SHORTCUT;
  }
  return trimmed;
}

function normalizeSearchShortcut(shortcut: string): string {
  const trimmed = shortcut.trim();
  if (!trimmed) {
    return DEFAULT_SEARCH_SHORTCUT;
  }

  if (LEGACY_SEARCH_SHORTCUTS.has(trimmed.toLowerCase())) {
    return DEFAULT_SEARCH_SHORTCUT;
  }

  return trimmed;
}

function sanitizeSettings(payload: UserSetting): UserSetting {
  const pickerPositionMode = pickerPositionModes.has(payload.pickerPositionMode)
    ? payload.pickerPositionMode
    : "mouse";
  const themeMode = themeModes.has(payload.themeMode) ? payload.themeMode : "system";

  return {
    ...structuredClone(payload),
    shortcut: normalizeMainShortcut(payload.shortcut),
    silentOnStartup: payload.launchOnStartup ? payload.silentOnStartup : false,
    pickerRecordLimit: Math.min(1000, Math.max(9, Math.trunc(payload.pickerRecordLimit || 50))),
    pickerPositionMode,
    themeMode,
    searchShortcut: normalizeSearchShortcut(payload.searchShortcut),
    searchShortcutEnabled: payload.searchShortcutEnabled,
  };
}

let items: ClipItemDetail[] = [
  {
    id: "demo-1",
    type: "text",
    contentPreview: "FloatPaste MVP 已开始落地，当前包含 SQLite、搜索协议与 Manager 基础界面。",
    fullText: "FloatPaste MVP 已开始落地，当前包含 SQLite、搜索协议与 Manager 基础界面。",
    searchText: "floatpaste mvp 已开始落地 当前包含 sqlite 搜索协议 与 manager 基础界面",
    sourceApp: "README.md",
    isFavorited: true,
    directoryCount: 0,
    createdAt: new Date(now - 1000 * 60 * 35).toISOString(),
    updatedAt: new Date(now - 1000 * 60 * 35).toISOString(),
    lastUsedAt: new Date(now - 1000 * 60 * 10).toISOString(),
    hash: "demo-hash-1",
    imagePath: null,
    imageWidth: null,
    imageHeight: null,
    imageFormat: null,
    fileSize: null,
    filePaths: [],
    fileCount: 0,
    totalSize: null,
  },
  {
    id: "demo-2",
    type: "text",
    contentPreview: "Manager 支持搜索、收藏、删除、编辑，Picker 与回贴执行留在下一阶段继续打通。",
    fullText: "Manager 支持搜索、收藏、删除、编辑，Picker 与回贴执行留在下一阶段继续打通。",
    searchText: "manager 支持 搜索 收藏 删除 编辑 picker 与 回贴 执行 留在 下一阶段 继续 打通",
    sourceApp: "架构设计与MVP技术方案.md",
    isFavorited: false,
    directoryCount: 0,
    createdAt: new Date(now - 1000 * 60 * 20).toISOString(),
    updatedAt: new Date(now - 1000 * 60 * 20).toISOString(),
    lastUsedAt: null,
    hash: "demo-hash-2",
    imagePath: null,
    imageWidth: null,
    imageHeight: null,
    imageFormat: null,
    fileSize: null,
    filePaths: [],
    fileCount: 0,
    totalSize: null,
  },
  {
    id: "demo-3",
    type: "image",
    contentPreview: "图片 (1920 × 1080, 2.4 MB)",
    fullText: "",
    searchText: "图片 1920 1080",
    sourceApp: "微信",
    isFavorited: false,
    directoryCount: 0,
    createdAt: new Date(now - 1000 * 60 * 60).toISOString(),
    updatedAt: new Date(now - 1000 * 60 * 60).toISOString(),
    lastUsedAt: null,
    hash: "demo-hash-3",
    imagePath: "images/demo-3.png",
    imageWidth: 1920,
    imageHeight: 1080,
    imageFormat: "png",
    fileSize: 2400000,
    filePaths: [],
    fileCount: 0,
    totalSize: null,
  },
  {
    id: "demo-4",
    type: "file",
    contentPreview: "文件: 产品需求文档.pdf",
    fullText: "",
    searchText: "产品需求文档.pdf",
    sourceApp: "文件资源管理器",
    isFavorited: true,
    fileCount: 1,
    directoryCount: 0,
    createdAt: new Date(now - 1000 * 60 * 60 * 24).toISOString(),
    updatedAt: new Date(now - 1000 * 60 * 60 * 24).toISOString(),
    lastUsedAt: new Date(now - 1000 * 60 * 120).toISOString(),
    hash: "demo-hash-4",
    filePaths: ["C:\\Users\\User\\Documents\\产品需求文档.pdf"],
    totalSize: 3500000,
    imagePath: null,
    imageWidth: null,
    imageHeight: null,
    imageFormat: null,
    fileSize: null,
  },
  {
    id: "demo-5",
    type: "file",
    contentPreview: "2 个文件，1 个文件夹",
    fullText: "",
    searchText: "会议纪要.docx 预算表.xlsx 素材目录",
    sourceApp: "Microsoft Teams",
    isFavorited: false,
    fileCount: 3,
    directoryCount: 1,
    createdAt: new Date(now - 1000 * 60 * 60 * 48).toISOString(),
    updatedAt: new Date(now - 1000 * 60 * 60 * 48).toISOString(),
    lastUsedAt: null,
    hash: "demo-hash-5",
    filePaths: [
      "C:\\Users\\User\\Downloads\\会议纪要.docx",
      "C:\\Users\\User\\Downloads\\预算表.xlsx",
      "C:\\Users\\User\\Downloads\\素材目录",
    ],
    totalSize: null,
    imagePath: null,
    imageWidth: null,
    imageHeight: null,
    imageFormat: null,
    fileSize: null,
  },
];

function getActivityTimestamp(item: ClipItemDetail): number {
  return Date.parse(item.lastUsedAt ?? item.createdAt);
}

function rankItems(query: SearchQuery): ClipItemDetail[] {
  const keyword = query.keyword.trim().toLowerCase();
  let result = [...items];

  if (query.filters.favoritedOnly) {
    result = result.filter((item) => item.isFavorited);
  }

  if (query.filters.clipType) {
    result = result.filter((item) => item.type === query.filters.clipType);
  }

  if (query.filters.sourceApp) {
    result = result.filter((item) => item.sourceApp === query.filters.sourceApp);
  }

  if (keyword) {
    result = result.filter((item) =>
      `${item.contentPreview} ${item.searchText} ${item.sourceApp ?? ""}`.toLowerCase().includes(keyword),
    );

    result.sort((left, right) => {
      const leftIndex = left.searchText.indexOf(keyword);
      const rightIndex = right.searchText.indexOf(keyword);
      if (leftIndex !== rightIndex) {
        return leftIndex - rightIndex;
      }

      const activityDiff = getActivityTimestamp(right) - getActivityTimestamp(left);
      if (activityDiff !== 0) {
        return activityDiff;
      }

      return Date.parse(right.createdAt) - Date.parse(left.createdAt);
    });
  } else {
    result.sort((left, right) => {
      const activityDiff = getActivityTimestamp(right) - getActivityTimestamp(left);
      if (activityDiff !== 0) {
        return activityDiff;
      }

      return Date.parse(right.createdAt) - Date.parse(left.createdAt);
    });
  }

  return result;
}

function toSummary(item: ClipItemDetail): ClipItemSummary {
  return {
    id: item.id,
    type: item.type,
    contentPreview: item.contentPreview,
    sourceApp: item.sourceApp,
    isFavorited: item.isFavorited,
    fileCount: item.fileCount,
    directoryCount: item.directoryCount,
    createdAt: item.createdAt,
    updatedAt: item.updatedAt,
    lastUsedAt: item.lastUsedAt,
    imagePath: item.imagePath,
    imageWidth: item.imageWidth,
    imageHeight: item.imageHeight,
    imageFormat: item.imageFormat,
    fileSize: item.fileSize,
  };
}

export async function mockListRecentItems(limit: number): Promise<ClipItemSummary[]> {
  return rankItems({
    keyword: "",
    filters: {},
    offset: 0,
    limit,
    sort: "recent_desc",
  })
    .slice(0, limit)
    .map(toSummary);
}

export async function mockListFavoriteItems(limit: number): Promise<ClipItemSummary[]> {
  return rankItems({
    keyword: "",
    filters: { favoritedOnly: true },
    offset: 0,
    limit,
    sort: "recent_desc",
  })
    .slice(0, limit)
    .map(toSummary);
}

export async function mockSearchItems(query: SearchQuery): Promise<SearchResult> {
  const result = rankItems(query);
  return {
    items: result.slice(query.offset, query.offset + query.limit).map(toSummary),
    total: result.length,
    offset: query.offset,
    limit: query.limit,
  };
}

export async function mockGetItemDetail(id: string): Promise<ClipItemDetail> {
  const item = items.find((entry) => entry.id === id);
  if (!item) {
    throw new Error("未找到对应剪贴记录");
  }

  return structuredClone(item);
}

export async function mockUpdateTextItem(id: string, text: string): Promise<ClipItemDetail> {
  const item = items.find((entry) => entry.id === id);
  if (!item) {
    throw new Error("未找到对应剪贴记录");
  }

  if (item.type !== "text") {
    throw new Error("只能编辑文本类型的记录");
  }

  item.fullText = text;
  item.searchText = text.toLowerCase();
  item.contentPreview = text.slice(0, 80);
  item.updatedAt = new Date().toISOString();
  return structuredClone(item);
}

export async function mockDeleteItem(id: string): Promise<void> {
  items = items.filter((item) => item.id !== id);
}

export async function mockSetItemFavorited(id: string, value: boolean): Promise<void> {
  const item = items.find((entry) => entry.id === id);
  if (!item) {
    throw new Error("未找到对应剪贴记录");
  }
  item.isFavorited = value;
  item.updatedAt = new Date().toISOString();
}

export async function mockPasteItem(id: string, option: PasteOption): Promise<PasteResult> {
  const item = items.find((entry) => entry.id === id);
  if (!item) {
    throw new Error("未找到对应剪贴记录");
  }

  const now = new Date().toISOString();
  item.lastUsedAt = now;
  item.updatedAt = now;

  return {
    success: true,
    code:
      option.restoreClipboardAfterPaste ? `${item.type}_clipboard_only_restore` : `${item.type}_clipboard_only`,
    message:
      item.type === "text"
        ? "浏览器预览模式下仅模拟写入文本剪贴板，系统注入将在 Tauri 环境执行。"
        : `浏览器预览模式下仅模拟写入${item.type === "image" ? "图片" : "文件"}剪贴板，系统注入将在 Tauri 环境执行。`,
  };
}

export async function mockGetSettings(): Promise<UserSetting> {
  return sanitizeSettings(settings);
}

export async function mockUpdateSettings(payload: UserSetting): Promise<UserSetting> {
  settings = sanitizeSettings(payload);
  return structuredClone(settings);
}

export async function mockPauseMonitoring(): Promise<UserSetting> {
  settings.pauseMonitoring = true;
  return structuredClone(settings);
}

export async function mockResumeMonitoring(): Promise<UserSetting> {
  settings.pauseMonitoring = false;
  return structuredClone(settings);
}

let settings: UserSetting = {
  shortcut: DEFAULT_MAIN_SHORTCUT,
  launchOnStartup: false,
  silentOnStartup: false,
  historyLimit: 1000,
  pickerRecordLimit: 50,
  pickerPositionMode: "mouse",
  excludedApps: ["KeePass.exe", "WindowsTerminal.exe"],
  restoreClipboardAfterPaste: true,
  pauseMonitoring: false,
  themeMode: "system",
  searchShortcut: DEFAULT_SEARCH_SHORTCUT,
  searchShortcutEnabled: true,
};

