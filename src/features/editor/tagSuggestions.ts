export const MAX_SUGGESTIONS = 8;

export function normalizeTagInput(value: string): string {
  return value.split(/\s+/).join(" ").trim();
}

export interface TagStat {
  name: string;
  itemCount: number;
}

/** 建议项：match=既有标签命中；exists=与已添加标签重复；create=作为新标签创建 */
export type TagSuggestion =
  | { kind: "match"; name: string; itemCount: number }
  | { kind: "exists"; name: string }
  | { kind: "create"; name: string };

/**
 * 由全部标签、已添加标签与当前输入计算建议列表。
 * - 空输入：返回常用标签（调用方数据已按使用次数降序）
 * - 非空输入：包含匹配（大小写不敏感），末尾追加"已添加/创建"状态项
 */
export function buildTagSuggestions(options: {
  allTags: TagStat[];
  current: string[];
  keyword: string;
}): TagSuggestion[] {
  const keyword = normalizeTagInput(options.keyword).toLowerCase();
  const added = new Set(options.current.map((name) => name.toLowerCase()));
  const pool = options.allTags.filter((tag) => !added.has(tag.name.toLowerCase()));
  const matched = keyword ? pool.filter((tag) => tag.name.toLowerCase().includes(keyword)) : pool;
  const suggestions: TagSuggestion[] = matched
    .slice(0, MAX_SUGGESTIONS)
    .map((tag) => ({ kind: "match" as const, name: tag.name, itemCount: tag.itemCount }));

  if (!keyword) {
    return suggestions;
  }
  if (added.has(keyword)) {
    // 展示已添加标签的规范写法，让用户看到与输入的差异
    const existing = options.current.find((name) => name.toLowerCase() === keyword);
    suggestions.push({ kind: "exists", name: existing ?? keyword });
    return suggestions;
  }
  // 全库已有仅大小写不同的标签时不提示"创建"：提交会被对齐到既有写法，选 match 项即可
  const canonicalExists = pool.some((tag) => tag.name.toLowerCase() === keyword);
  if (!canonicalExists) {
    suggestions.push({ kind: "create", name: normalizeTagInput(options.keyword) });
  }
  return suggestions;
}
