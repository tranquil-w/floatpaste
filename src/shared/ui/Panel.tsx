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
      className={`rounded-lg border border-cp-surface1/30 bg-cp-mantle/70 p-5 sm:p-6 shadow-sm backdrop-blur-md transition-all duration-250 hover:border-cp-surface1/50 hover:bg-cp-mantle/85 ${className}`}
    >
      {children}
    </section>
  );
}
