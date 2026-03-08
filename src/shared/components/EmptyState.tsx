interface EmptyStateProps {
  title: string;
  description: string;
}

export function EmptyState({ title, description }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center rounded-3xl border border-dashed border-slate-300 bg-slate-50/50 px-6 py-12 text-center transition-colors duration-300 hover:border-slate-400 hover:bg-slate-50">
      <div className="mb-5 flex h-12 w-12 items-center justify-center rounded-full bg-slate-100 ring-8 ring-white">
        <div className="h-4 w-4 rounded-sm bg-slate-300" />
      </div>
      <h3 className="font-display text-lg font-medium text-slate-800">{title}</h3>
      <p className="mt-2 max-w-sm text-sm leading-relaxed text-slate-500">{description}</p>
    </div>
  );
}
