interface EmptyStateProps {
  title: string;
  description: string;
}

export function EmptyState({ title, description }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center rounded-[24px] border border-dashed border-[rgba(var(--cp-surface1-rgb),0.4)] bg-[rgba(var(--cp-mantle-rgb),0.5)] px-6 py-12 text-center transition-all duration-300 hover:border-[rgba(var(--cp-surface1-rgb),0.6)] hover:bg-[rgba(var(--cp-mantle-rgb),0.7)]">
      <div className="mb-5 flex h-12 w-12 items-center justify-center rounded-full bg-[rgba(var(--cp-surface0-rgb),0.6)] ring-8 ring-[color:var(--cp-window-shell)] shadow-sm">
        <div className="h-4 w-4 rounded-sm bg-[rgba(var(--cp-surface1-rgb),0.8)]" />
      </div>
      <h3 className="font-display text-lg font-bold tracking-tight text-[color:var(--cp-text-primary)]">{title}</h3>
      <p className="mt-2 max-w-sm text-sm font-medium leading-relaxed text-[color:var(--cp-text-secondary)]">{description}</p>
    </div>
  );
}
