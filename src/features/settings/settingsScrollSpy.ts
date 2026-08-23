import type { SettingsSectionId } from "./settingsSections";

/** 分组标题滚动定位时与视口顶保持的距离，同时是 scrollspy 的偏移线 */
export const SCROLL_OFFSET = 96;

/** 分组元素只需能报告自身顶部位置即可，便于在无 DOM 环境下测试 */
export type SectionElementBox = {
  getBoundingClientRect: () => { top: number };
};

export type ScrollSpyViewport = {
  scrollY: number;
  viewportHeight: number;
  scrollHeight: number;
};

export function getActiveSectionId(
  orderedSectionIds: readonly SettingsSectionId[],
  sectionElements: ReadonlyMap<SettingsSectionId, SectionElementBox>,
  viewport: ScrollSpyViewport,
) {
  const firstSectionId = orderedSectionIds[0] ?? "general";
  const lastSectionId =
    orderedSectionIds[orderedSectionIds.length - 1] ?? firstSectionId;

  // 滚动到底时最后一个分组可能没有空间越过偏移线（点击它会被钳制在文档底部），
  // 此时直接高亮最后一个分组，避免高亮被上一分组抢占
  const isAtBottom =
    viewport.scrollY + viewport.viewportHeight >= viewport.scrollHeight - 1;
  if (isAtBottom && sectionElements.has(lastSectionId)) {
    return lastSectionId;
  }

  // 高亮顶部偏移线经过的最后一个分组（分组按文档顺序排列）
  let candidateId: SettingsSectionId | null = null;
  for (const sectionId of orderedSectionIds) {
    const element = sectionElements.get(sectionId);
    if (element && element.getBoundingClientRect().top <= SCROLL_OFFSET) {
      candidateId = sectionId;
    }
  }

  return candidateId ?? firstSectionId;
}
