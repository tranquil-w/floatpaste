type FileLikeClip = {
  type: string;
  fileCount?: number | null;
  directoryCount?: number | null;
};

function resolveFileCounts(clip: FileLikeClip) {
  const fileCount = Math.max(0, clip.fileCount ?? 0);
  const directoryCount = Math.min(fileCount, Math.max(0, clip.directoryCount ?? 0));
  return { fileCount, directoryCount };
}

export function getClipTypeLabel(clip: FileLikeClip): string {
  if (clip.type === "text") {
    return "文本";
  }

  if (clip.type === "image") {
    return "图片";
  }

  if (clip.type !== "file") {
    return "未知";
  }

  const { fileCount, directoryCount } = resolveFileCounts(clip);
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

/** 文件大小的人读格式；非正数返回 null，由调用方决定是否省略该字段 */
export function formatFileSize(bytes: number | null | undefined): string | null {
  if (!bytes || bytes <= 0) {
    return null;
  }

  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  const digits = unitIndex === 0 ? 0 : value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${units[unitIndex]}`;
}
