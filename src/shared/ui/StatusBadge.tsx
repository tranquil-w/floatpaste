interface StatusBadgeProps {
  tone: "running" | "paused" | "muted";
  children: string;
}

const toneClassMap: Record<StatusBadgeProps["tone"], string> = {
  running: "bg-[color:var(--pg-success-subtle)] text-[color:var(--pg-success-fg)] ring-1 ring-inset ring-[color:var(--pg-success-fg)]/20",
  paused: "bg-[color:var(--pg-warning-subtle)] text-[color:var(--pg-warning-fg)] ring-1 ring-inset ring-[color:var(--pg-warning-fg)]/20",
  muted: "bg-[color:var(--pg-neutral-3)] text-[color:var(--pg-fg-subtle)] ring-1 ring-inset ring-[color:var(--pg-neutral-6)]/40",
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
