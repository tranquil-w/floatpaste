interface StatusBadgeProps {
  tone: "running" | "paused" | "muted";
  children: string;
}

const toneClassMap: Record<StatusBadgeProps["tone"], string> = {
  running: "bg-cp-green/15 text-cp-green ring-1 ring-inset ring-cp-green/30",
  paused: "bg-cp-yellow/15 text-cp-yellow ring-1 ring-inset ring-cp-yellow/30",
  muted: "bg-cp-surface0 text-cp-subtext0 ring-1 ring-inset ring-cp-surface2",
};

export function StatusBadge({ tone, children }: StatusBadgeProps) {
  return (
    <span
      className={`inline-flex items-center rounded-full px-2.5 py-1 text-[11px] font-semibold tracking-wide ${toneClassMap[tone]}`}
    >
      {children}
    </span>
  );
}
