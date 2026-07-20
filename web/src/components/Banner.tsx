export function ErrorBanner({ message }: { message: string }) {
  return (
    <div className="my-3 flex items-center gap-2 rounded-lg border border-[var(--fail)] bg-[var(--fail-soft)] px-3 py-2 font-semibold text-[var(--fail)]">
      <span aria-hidden>⚠</span>
      {message}
    </div>
  );
}

export function WarnBanner({ message }: { message: string }) {
  return (
    <div className="my-2 flex items-center gap-2 rounded-lg border border-[var(--diff-fg)]/30 bg-[var(--diff-bg)] px-3 py-2 text-[var(--diff-fg)]">
      <span aria-hidden>💡</span>
      {message}
    </div>
  );
}
