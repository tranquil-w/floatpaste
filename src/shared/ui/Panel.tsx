import type { PropsWithChildren } from "react";

interface PanelProps extends PropsWithChildren {
  className?: string;
}

export function Panel({ children, className = "" }: PanelProps) {
  return (
    <section
      className={`rounded-3xl border border-[color:var(--cp-border-soft)] bg-[color:var(--cp-panel-surface)]/70 p-5 sm:p-6 shadow-panel backdrop-blur-xl transition-all duration-300 hover:border-[color:var(--cp-border-strong)]/30 hover:bg-[color:var(--cp-panel-surface)]/85 ${className}`}
    >
      {children}
    </section>
  );
}
