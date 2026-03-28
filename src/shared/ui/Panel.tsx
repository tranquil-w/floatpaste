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
      className={`rounded-lg border border-pg-border-muted bg-pg-canvas-subtle p-5 sm:p-6 shadow-pg-sm transition-all duration-250 hover:border-pg-border-default ${className}`}
    >
      {children}
    </section>
  );
}
