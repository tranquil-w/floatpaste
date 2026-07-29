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
