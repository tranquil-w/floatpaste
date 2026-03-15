import type { PropsWithChildren } from "react";

interface PanelProps extends PropsWithChildren {
  className?: string;
}

export function Panel({ children, className = "" }: PanelProps) {
  return (
    <section
      className={`rounded-3xl border border-[rgba(var(--cp-surface1-rgb),0.3)] bg-[rgba(var(--cp-mantle-rgb),0.7)] p-5 sm:p-6 shadow-panel backdrop-blur-xl transition-all duration-300 hover:border-[rgba(var(--cp-surface1-rgb),0.5)] hover:bg-[rgba(var(--cp-mantle-rgb),0.85)] ${className}`}
    >
      {children}
    </section>
  );
}
