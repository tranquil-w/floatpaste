interface StatusBadgeProps {
  tone: "running" | "paused" | "muted";
  children: string;
}

const toneClassMap: Record<StatusBadgeProps["tone"], string> = {
  running: "bg-green-100 text-green-800",
  paused: "bg-amber-100 text-amber-800",
  muted: "bg-slate-200 text-slate-700",
};

export function StatusBadge({ tone, children }: StatusBadgeProps) {
  return (
    <span
      className={`inline-flex rounded-full px-3 py-1 text-xs font-semibold tracking-wide ${toneClassMap[tone]}`}
    >
      {children}
    </span>
  );
}
