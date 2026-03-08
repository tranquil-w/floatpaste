export type SearchSort = "relevance_desc" | "recent_desc";

export interface SearchFilters {
  favoritedOnly?: boolean;
  sourceApp?: string | null;
  includeDeleted?: false;
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
  contentPreview: string;
  sourceApp: string | null;
  isFavorited: boolean;
  createdAt: string;
  updatedAt: string;
  lastUsedAt: string | null;
}

export interface ClipItemDetail extends ClipItemSummary {
  fullText: string;
  searchText: string;
  hash: string;
  type: "text";
}

export interface SearchResult {
  items: ClipItemSummary[];
  total: number;
  offset: number;
  limit: number;
}

export interface PasteOption {
  restoreClipboardAfterPaste: boolean;
}

export interface PasteResult {
  success: boolean;
  code: string;
  message: string;
}
