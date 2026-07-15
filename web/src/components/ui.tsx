import type {
  InputHTMLAttributes,
  LabelHTMLAttributes,
  ButtonHTMLAttributes,
  ReactNode,
} from "react";

export function Label(props: LabelHTMLAttributes<HTMLLabelElement>) {
  return <label {...props} className="mb-1 mt-3 block text-[13px] font-semibold" />;
}

export function Input(props: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={`min-w-[220px] rounded-md border border-[var(--border)] bg-[var(--bg)] px-2 py-1.5 text-sm text-[var(--fg)] ${props.className ?? ""}`}
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
          ? "cursor-pointer rounded-md border border-[var(--accent)] bg-transparent px-3.5 py-2 text-sm font-semibold text-[var(--accent)]"
          : "cursor-pointer rounded-md border border-[var(--accent)] bg-[var(--accent)] px-3.5 py-2 text-sm font-semibold text-white disabled:opacity-50"
      }
    />
  );
}

export function TableWrap({ children }: { children: ReactNode }) {
  return (
    <div className="overflow-x-auto rounded-md border border-[var(--border)]">
      <table className="w-full border-collapse text-sm">{children}</table>
    </div>
  );
}
