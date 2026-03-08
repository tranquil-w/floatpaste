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

let settings: UserSetting = {
  shortcut: "Ctrl+`",
  launchOnStartup: false,
  historyLimit: 1000,
  excludedApps: ["KeePass.exe", "WindowsTerminal.exe"],
  restoreClipboardAfterPaste: true,
  pauseMonitoring: false,
};

let items: ClipItemDetail[] = [
  {
    id: "demo-1",
    type: "text",
    contentPreview: "FloatPaste MVP 已开始落地，当前包含 SQLite、搜索协议与 Manager 基础界面。",
    fullText: "FloatPaste MVP 已开始落地，当前包含 SQLite、搜索协议与 Manager 基础界面。",
    searchText: "floatpaste mvp 已开始落地 当前包含 sqlite 搜索协议 与 manager 基础界面",
    sourceApp: "README.md",
    isFavorited: true,
    createdAt: new Date(now - 1000 * 60 * 35).toISOString(),
    updatedAt: new Date(now - 1000 * 60 * 35).toISOString(),
    lastUsedAt: new Date(now - 1000 * 60 * 10).toISOString(),
    hash: "demo-hash-1",
  },
  {
    id: "demo-2",
    type: "text",
    contentPreview: "Manager 支持搜索、收藏、删除、编辑，Picker 与回贴执行留在下一阶段继续打通。",
    fullText: "Manager 支持搜索、收藏、删除、编辑，Picker 与回贴执行留在下一阶段继续打通。",
    searchText: "manager 支持 搜索 收藏 删除 编辑 picker 与 回贴 执行 留在 下一阶段 继续 打通",
    sourceApp: "架构设计与MVP技术方案.md",
    isFavorited: false,
    createdAt: new Date(now - 1000 * 60 * 20).toISOString(),
    updatedAt: new Date(now - 1000 * 60 * 20).toISOString(),
    lastUsedAt: null,
    hash: "demo-hash-2",
  },
];

function rankItems(query: SearchQuery): ClipItemDetail[] {
  const keyword = query.keyword.trim().toLowerCase();
  let result = [...items];

  if (query.filters.favoritedOnly) {
    result = result.filter((item) => item.isFavorited);
  }

  if (query.filters.sourceApp) {
    result = result.filter((item) => item.sourceApp === query.filters.sourceApp);
  }

  if (keyword) {
    result = result.filter((item) =>
      `${item.fullText} ${item.searchText} ${item.sourceApp ?? ""}`.toLowerCase().includes(keyword),
    );

    result.sort((left, right) => {
      if (left.isFavorited !== right.isFavorited) {
        return Number(right.isFavorited) - Number(left.isFavorited);
      }

      const leftIndex = left.searchText.indexOf(keyword);
      const rightIndex = right.searchText.indexOf(keyword);
      if (leftIndex !== rightIndex) {
        return leftIndex - rightIndex;
      }

      return Date.parse(right.updatedAt) - Date.parse(left.updatedAt);
    });
  } else {
    result.sort((left, right) => {
      if (left.isFavorited !== right.isFavorited) {
        return Number(right.isFavorited) - Number(left.isFavorited);
      }

      const leftUsed = left.lastUsedAt ? Date.parse(left.lastUsedAt) : 0;
      const rightUsed = right.lastUsedAt ? Date.parse(right.lastUsedAt) : 0;
      if (leftUsed !== rightUsed) {
        return rightUsed - leftUsed;
      }

      return Date.parse(right.createdAt) - Date.parse(left.createdAt);
    });
  }

  return result;
}

function toSummary(item: ClipItemDetail): ClipItemSummary {
  return {
    id: item.id,
    contentPreview: item.contentPreview,
    sourceApp: item.sourceApp,
    isFavorited: item.isFavorited,
    createdAt: item.createdAt,
    updatedAt: item.updatedAt,
    lastUsedAt: item.lastUsedAt,
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

export async function mockPasteItem(_id: string, option: PasteOption): Promise<PasteResult> {
  return {
    success: true,
    code: option.restoreClipboardAfterPaste ? "clipboard_only_restore" : "clipboard_only",
    message: "浏览器预览模式下仅模拟写入剪贴板，系统注入将在 Tauri 环境执行。",
  };
}

export async function mockGetSettings(): Promise<UserSetting> {
  return structuredClone(settings);
}

export async function mockUpdateSettings(payload: UserSetting): Promise<UserSetting> {
  settings = structuredClone(payload);
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
