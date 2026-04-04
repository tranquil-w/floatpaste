import type { ClipItemSummary } from "../../shared/types/clips";

type BuildTooltipHtmlOptions = {
  imageUrl?: string | null;
  requestId?: number;
};

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export function escapeHtmlAttribute(text: string): string {
  return escapeHtml(text).replace(/'/g, "&#39;");
}

function getTooltipTypeLabel(item: ClipItemSummary): string {
  if (item.type === "text") {
    return "文本";
  }

  if (item.type === "image") {
    return "图片";
  }

  if (item.type !== "file") {
    return "未知";
  }

  const fileCount = Math.max(0, item.fileCount ?? 0);
  const directoryCount = Math.min(fileCount, Math.max(0, item.directoryCount ?? 0));
  if (!fileCount) {
    return "文件";
  }
  if (directoryCount === fileCount) {
    return "文件夹";
  }
  if (directoryCount > 0) {
    return "文件/文件夹";
  }
  return "文件";
}

function formatAbsoluteDateTime(date: Date): string {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "numeric",
  }).format(date);
}

function formatTooltipDateTime(value: string | null): string {
  if (!value) {
    return "未使用";
  }

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }

  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  if (diffMs < 0) {
    return formatAbsoluteDateTime(date);
  }

  const diffSec = Math.floor(diffMs / 1000);
  const diffMin = Math.floor(diffSec / 60);
  const diffHour = Math.floor(diffMin / 60);
  const diffDay = Math.floor(diffHour / 24);

  if (diffSec < 60) {
    return "刚刚";
  }
  if (diffMin < 60) {
    return `${diffMin} 分钟前`;
  }
  if (diffHour < 24) {
    return `${diffHour} 小时前`;
  }
  if (diffDay === 1) {
    return "昨天";
  }
  if (diffDay < 7) {
    return `${diffDay} 天前`;
  }

  return formatAbsoluteDateTime(date);
}

export function buildTooltipHtml(item: ClipItemSummary, options: BuildTooltipHtmlOptions = {}): string {
  const metaParts: string[] = [];
  const escapedSource = escapeHtml(item.sourceApp ?? "未知来源");

  metaParts.push(`<span class="meta-badge">${escapeHtml(getTooltipTypeLabel(item))}</span>`);
  if (item.type === "image" && item.imageWidth && item.imageHeight) {
    metaParts.push(`<span class="meta-size">${item.imageWidth} × ${item.imageHeight}</span>`);
  }
  if (item.type === "image" && item.imageFormat) {
    metaParts.push(`<span class="meta-format">${escapeHtml(item.imageFormat)}</span>`);
  }
  metaParts.push(`<span class="meta-source">${escapedSource}</span>`);
  metaParts.push(
    `<span class="meta-time">${escapeHtml(formatTooltipDateTime(item.lastUsedAt ?? item.createdAt))}</span>`,
  );

  if (item.type === "image" && options.imageUrl) {
    const requestId = options.requestId ?? 0;
    const fallbackContent = item.tooltipText || item.contentPreview || "";
    return [
      `<div class="tooltip-image-preview"><img src="${escapeHtmlAttribute(options.imageUrl)}" alt="" data-request-id="${requestId}" data-fallback-content="${escapeHtmlAttribute(fallbackContent)}" /></div>`,
      `<div class="tooltip-meta">${metaParts.join("")}</div>`,
    ].join("");
  }

  const escapedContent = item.tooltipText || item.contentPreview || "";
  return `<div class="tooltip-content">${escapeHtml(escapedContent)}</div><div class="tooltip-meta">${metaParts.join("")}</div>`;
}
