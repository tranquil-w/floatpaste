import type { PropsWithChildren } from "react";

interface PanelProps extends PropsWithChildren {
  className?: string;
}

export function Panel({ children, className = "" }: PanelProps) {
  return (
    <section
      className={`rounded-3xl border border-cp-surface0/60 bg-cp-mantle/70 p-5 sm:p-6 shadow-panel backdrop-blur-xl transition-all duration-300 hover:border-cp-surface0 hover:bg-cp-mantle/80 ${className}`}
    >
      {children}
    </section>
  );
}
