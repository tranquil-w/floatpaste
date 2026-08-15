export type SearchSort = "relevance_desc" | "recent_desc";
export type ClipType = "text" | "image" | "file";
export type SearchQuickFilter = "all" | "favorite" | ClipType | "tag";

export interface SearchFilters {
  favoritedOnly?: boolean;
  clipType?: ClipType;
  sourceApp?: string | null;
  includeDeleted?: false;
  /** 按标签筛选，AND 语义；空数组等价于未筛选 */
  tagNames?: string[] | null;
}

export interface TagInfo {
  name: string;
  itemCount: number;
  createdAt: string;
}

export interface SearchQuery {
  keyword: string;
  filters: SearchFilters;
  offset: number;
  limit: number;
  sort: SearchSort;
}

export interface ClipItemSummary {
  id: string;
  type: ClipType;
  contentPreview: string;
  tooltipText?: string | null;
  sourceApp: string | null;
  isFavorited: boolean;
  fileCount: number;
  directoryCount: number;
  createdAt: string;
  updatedAt: string;
  lastUsedAt: string | null;
  imagePath: string | null;
  imageWidth: number | null;
  imageHeight: number | null;
  imageFormat: string | null;
  fileSize: number | null;
  tags: string[];
}

// 所有字段都放在一个基础接口上，通过 type 区分行为
export interface ClipItemDetail {
  id: string;
  type: ClipType;
  contentPreview: string;
  sourceApp: string | null;
  isFavorited: boolean;
  createdAt: string;
  updatedAt: string;
  lastUsedAt: string | null;
  searchText: string;
  hash: string;

  fullText: string;

  imagePath: string | null;
  imageWidth: number | null;
  imageHeight: number | null;
  imageFormat: string | null;
  fileSize: number | null;

  filePaths: string[];
  fileCount: number;
  directoryCount: number;
  totalSize: number | null;
  tags: string[];
}

export interface SearchResult {
  items: ClipItemSummary[];
  total: number;
  offset: number;
  limit: number;
}

export interface PasteOption {
  restoreClipboardAfterPaste: boolean;
  pasteToTarget?: boolean;
  asFile?: boolean;
}

export interface PasteResult {
  success: boolean;
  code: string;
  message: string;
}
