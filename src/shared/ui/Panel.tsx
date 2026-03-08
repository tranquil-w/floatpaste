import type { PropsWithChildren } from "react";

interface PanelProps extends PropsWithChildren {
  className?: string;
}

export function Panel({ children, className = "" }: PanelProps) {
  return (
    <section
      className={`rounded-3xl border border-white/60 bg-white/80 p-5 shadow-panel backdrop-blur ${className}`}
    >
      {children}
    </section>
  );
}
