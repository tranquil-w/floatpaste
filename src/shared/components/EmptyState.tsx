interface EmptyStateProps {
  title: string;
  description: string;
}

export function EmptyState({ title, description }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center rounded-[24px] border border-dashed border-[color:var(--cp-border-soft)] bg-[color:var(--cp-panel-surface)]/50 px-6 py-12 text-center transition-all duration-300 hover:border-[color:var(--cp-border-strong)]/40 hover:bg-[color:var(--cp-panel-surface)]/70">
      <div className="mb-5 flex h-12 w-12 items-center justify-center rounded-full bg-[color:var(--cp-control-surface)] ring-8 ring-[color:var(--cp-window-shell)] shadow-sm">
        <div className="h-4 w-4 rounded-sm bg-[color:var(--cp-border-strong)]" />
      </div>
      <h3 className="font-display text-lg font-bold tracking-tight text-[color:var(--cp-text-primary)]">{title}</h3>
      <p className="mt-2 max-w-sm text-sm font-medium leading-relaxed text-[color:var(--cp-text-secondary)]">{description}</p>
    </div>
  );
}
