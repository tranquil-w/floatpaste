import type { PropsWithChildren } from "react";

interface PanelProps extends PropsWithChildren {
  className?: string;
  "aria-label"?: string;
  "aria-labelledby"?: string;
}

export function Panel({ children, className = "", "aria-label": ariaLabel, "aria-labelledby": ariaLabelledBy }: PanelProps) {
  return (
    <section
      aria-label={ariaLabel}
      aria-labelledby={ariaLabelledBy}
      className={`rounded-lg border border-[color:var(--pg-border-muted)] bg-[color:var(--pg-canvas-subtle)] p-5 sm:p-6 shadow-pg-sm transition-all duration-250 hover:border-[color:var(--pg-border-default)] ${className}`}
    >
      {children}
    </section>
  );
}
