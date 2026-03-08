interface StatusBadgeProps {
  tone: "running" | "paused" | "muted";
  children: string;
}

const toneClassMap: Record<StatusBadgeProps["tone"], string> = {
  running: "bg-green-50 text-green-700 ring-1 ring-inset ring-green-600/20",
  paused: "bg-amber-50 text-amber-700 ring-1 ring-inset ring-amber-600/20",
  muted: "bg-slate-50 text-slate-600 ring-1 ring-inset ring-slate-500/10",
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
