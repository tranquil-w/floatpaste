function formatAbsoluteDateTime(date: Date): string {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "numeric",
  }).format(date);
}

export function formatDateTime(value: string | null): string {
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

  // 超过一周的时间，显示为简短日期格式
  return formatAbsoluteDateTime(date);
}
