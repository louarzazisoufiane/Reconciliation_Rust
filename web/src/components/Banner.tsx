export function ErrorBanner({ message }: { message: string }) {
  return (
    <div className="my-3 rounded-md border border-[var(--fail)] px-3 py-2 font-semibold text-[var(--fail)]">
      {message}
    </div>
  );
}

export function WarnBanner({ message }: { message: string }) {
  return (
    <div className="my-2 rounded-md bg-[var(--diff-bg)] px-3 py-2 text-[var(--diff-fg)]">
      {message}
    </div>
  );
}
