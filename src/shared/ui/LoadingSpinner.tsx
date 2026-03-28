interface LoadingSpinnerProps {
  size?: "sm" | "md" | "lg";
  text?: string;
}

const sizeClasses = {
  sm: "h-4 w-4 border-2",
  md: "h-6 w-6 border-2",
  lg: "h-8 w-8 border-3",
};

export function LoadingSpinner({ size = "md", text }: LoadingSpinnerProps) {
  return (
    <div
      className="flex flex-col items-center justify-center gap-3"
      role="status"
      aria-live="polite"
      aria-busy="true"
    >
      <div
        className={`${sizeClasses[size]} animate-spin rounded-full border-[color:var(--pg-neutral-6)] border-t-[color:var(--pg-accent-fg)]`}
        aria-hidden="true"
      />
      {text && (
        <span className="text-sm font-medium text-[color:var(--pg-fg-muted)]">
          {text}
        </span>
      )}
      <span className="sr-only">加载中...</span>
    </div>
  );
}
