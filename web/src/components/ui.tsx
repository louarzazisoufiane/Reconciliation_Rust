import type {
  InputHTMLAttributes,
  LabelHTMLAttributes,
  ButtonHTMLAttributes,
  ReactNode,
} from "react";

export function Label(props: LabelHTMLAttributes<HTMLLabelElement>) {
  return (
    <label
      {...props}
      className="mb-1 mt-3 block text-[13px] font-semibold text-[var(--fg)]"
    />
  );
}

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={`min-w-[220px] rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-sm text-[var(--fg)] shadow-[var(--shadow-sm)] placeholder:text-[var(--muted)] ${props.className ?? ""}`}
    />
  );
}

export function Button({
  secondary,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { secondary?: boolean }) {
  return (
    <button
      {...props}
      className={
        secondary
          ? "cursor-pointer rounded-lg border border-[var(--border)] bg-[var(--surface)] px-4 py-2 text-sm font-semibold text-[var(--fg)] shadow-[var(--shadow-sm)] hover:bg-[var(--surface-2)] disabled:opacity-50"
          : "cursor-pointer rounded-lg border-0 bg-[var(--accent)] px-4 py-2 text-sm font-semibold text-white shadow-[var(--shadow-sm)] hover:bg-[var(--accent-hover)] disabled:opacity-50"
      }
    />
  );
}

export function TableWrap({ children }: { children: ReactNode }) {
  return (
    <div className="overflow-x-auto rounded-xl border border-[var(--border)] bg-[var(--surface)] shadow-[var(--shadow-md)]">
      <table className="w-full border-collapse text-sm">{children}</table>
    </div>
  );
}
