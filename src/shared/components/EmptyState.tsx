interface EmptyStateProps {
  title: string;
  description: string;
}

export function EmptyState({ title, description }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center rounded-3xl border border-dashed border-cp-surface0 bg-cp-mantle/50 px-6 py-12 text-center transition-colors duration-300 hover:border-cp-surface1 hover:bg-cp-mantle/60">
      <div className="mb-5 flex h-12 w-12 items-center justify-center rounded-full bg-cp-surface0 ring-8 ring-cp-base">
        <div className="h-4 w-4 rounded-sm bg-cp-surface2" />
      </div>
      <h3 className="font-display text-lg font-medium text-cp-text">{title}</h3>
      <p className="mt-2 max-w-sm text-sm leading-relaxed text-cp-subtext0">{description}</p>
    </div>
  );
}
