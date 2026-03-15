interface StatusBadgeProps {
  tone: "running" | "paused" | "muted";
  children: string;
}

const toneClassMap: Record<StatusBadgeProps["tone"], string> = {
  running: "bg-[color:var(--cp-green)]/15 text-[color:var(--cp-green)] ring-1 ring-inset ring-[color:var(--cp-green)]/30",
  paused: "bg-[color:var(--cp-accent-warm)]/15 text-[color:var(--cp-accent-warm)] ring-1 ring-inset ring-[color:var(--cp-accent-warm)]/30",
  muted: "bg-[color:var(--cp-control-surface)]/60 text-[color:var(--cp-text-muted)] ring-1 ring-inset ring-[color:var(--cp-border-soft)]",
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
