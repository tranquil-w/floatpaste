type ClipTypeIconProps = {
  type: string;
  className?: string;
};

const iconStrokeProps = {
  fill: "none",
  stroke: "currentColor",
  strokeLinecap: "round",
  strokeLinejoin: "round",
  strokeWidth: 1.8,
  viewBox: "0 0 24 24",
} as const;

/** 剪贴条目类型图标：文本/图片/文件共用一套 SVG，替代散落的汉字 glyph。 */
export function ClipTypeIcon({ type, className }: ClipTypeIconProps) {
  const shared = { ...iconStrokeProps, "aria-hidden": true, className } as const;

  if (type === "image") {
    return (
      <svg {...shared}>
        <rect height="14" rx="2.5" width="18" x="3" y="5" />
        <path d="m5 16 4.2-4.2a1.5 1.5 0 0 1 2.1 0L15 15.5" />
        <path d="m13 13 1.7-1.7a1.5 1.5 0 0 1 2.1 0L21 15.5" />
        <circle cx="9" cy="9.5" r="1.2" />
      </svg>
    );
  }

  if (type === "file") {
    return (
      <svg {...shared}>
        <path d="M6 3.75h7L18.25 9v11.25H6z" />
        <path d="M13 3.75V9h5.25" />
      </svg>
    );
  }

  return (
    <svg {...shared}>
      <path d="M5 6.25h14M5 11h11M5 15.75h14" />
    </svg>
  );
}
