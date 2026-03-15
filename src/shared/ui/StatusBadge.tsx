interface StatusBadgeProps {
  tone: "running" | "paused" | "muted";
  children: string;
}

const toneClassMap: Record<StatusBadgeProps["tone"], string> = {
  running: "bg-[rgba(var(--cp-green-rgb),0.15)] text-[color:var(--cp-green)] ring-1 ring-inset ring-[rgba(var(--cp-green-rgb),0.3)]",
  paused: "bg-[rgba(var(--cp-yellow-rgb),0.15)] text-[color:var(--cp-accent-warm)] ring-1 ring-inset ring-[rgba(var(--cp-yellow-rgb),0.3)]",
  muted: "bg-[rgba(var(--cp-surface0-rgb),0.6)] text-[color:var(--cp-text-muted)] ring-1 ring-inset ring-[rgba(var(--cp-surface1-rgb),0.4)]",
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
