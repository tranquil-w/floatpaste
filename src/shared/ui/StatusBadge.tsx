interface StatusBadgeProps {
  tone: "running" | "paused" | "muted";
  children: string;
}

const toneClassMap: Record<StatusBadgeProps["tone"], string> = {
  running: "bg-pg-success-subtle text-pg-success-fg ring-1 ring-inset ring-pg-success-fg/20",
  paused: "bg-pg-warning-subtle text-pg-warning-fg ring-1 ring-inset ring-pg-warning-fg/20",
  muted: "bg-pg-neutral-3 text-pg-fg-subtle ring-1 ring-inset ring-pg-neutral-6/40",
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
