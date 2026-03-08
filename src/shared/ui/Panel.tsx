import type { PropsWithChildren } from "react";

interface PanelProps extends PropsWithChildren {
  className?: string;
}

export function Panel({ children, className = "" }: PanelProps) {
  return (
    <section
      className={`rounded-3xl border border-white/60 bg-white/70 p-5 sm:p-6 shadow-panel backdrop-blur transition-all duration-300 ${className}`}
    >
      {children}
    </section>
  );
}
