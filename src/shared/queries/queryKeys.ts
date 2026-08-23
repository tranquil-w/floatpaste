import type { SearchQuery } from "../types/clips";

/**
 * 全部查询键的单一来源：失效与缓存写入必须引用这里的键，
 * 避免字符串字面量散落各处后失配导致缓存无法失效。
 * 前缀键（复数形式）用于 invalidateQueries/setQueriesData 的前缀匹配。
 */
export const queryKeys = {
  clipDetail: (id: string) => ["detail", id] as const,
  clipDetails: ["detail"],
  searchRecent: (query: SearchQuery) => ["search-recent", query] as const,
  searchRecents: ["search-recent"],
  searchQuery: (query: SearchQuery) => ["search-query", query] as const,
  searchQueries: ["search-query"],
  pickerRecent: (limit: number) => ["picker-recent", limit] as const,
  pickerRecents: ["picker-recent"],
  tags: ["tags"],
} as const;
