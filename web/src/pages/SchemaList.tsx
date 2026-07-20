import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import type { SchemaInfo } from "../types";
import { ErrorBanner } from "../components/Banner";
import { Button, TableWrap } from "../components/ui";

export default function SchemaList() {
  const [schemas, setSchemas] = useState<SchemaInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [deleting, setDeleting] = useState<string | null>(null);

  useEffect(() => {
    api
      .listSchemas()
      .then(setSchemas)
      .catch((e) => setError(e.message));
  }, []);

  async function onDelete(name: string) {
    if (
      !window.confirm(
        `Delete schema "${name}" and all of its versions? This cannot be undone.`,
      )
    ) {
      return;
    }
    setDeleting(name);
    setError(null);
    try {
      await api.deleteSchema(name);
      setSchemas((prev) => (prev ? prev.filter((s) => s.name !== name) : prev));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setDeleting(null);
    }
  }

  return (
    <div>
      <header className="mb-5">
        <h1 className="font-display text-2xl font-semibold text-[var(--fg)]">
          Schema library
        </h1>
        <div className="text-sm text-[var(--muted)]">{schemas?.length ?? "…"} schema(s)</div>
      </header>
      {error && <ErrorBanner message={error} />}
      <p className="mb-4">
        <Link to="/schemas/new">
          <Button type="button">+ New schema</Button>
        </Link>
      </p>
      {schemas && schemas.length === 0 && (
        <p className="text-[var(--muted)]">No schemas yet — create one to get started.</p>
      )}
      {schemas && schemas.length > 0 && (
        <TableWrap>
          <thead>
            <tr className="border-b border-[var(--border)] bg-[var(--surface-2)] text-left text-xs uppercase tracking-wide text-[var(--muted)]">
              <th className="p-3">Name</th>
              <th className="p-3">Latest</th>
              <th className="p-3">Fields</th>
              <th className="p-3">Created</th>
              <th className="p-3"></th>
            </tr>
          </thead>
          <tbody>
            {schemas.map((s) => (
              <tr key={s.name} className="border-b border-[var(--border)] last:border-0">
                <td className="p-3 font-medium">{s.name}</td>
                <td className="p-3">v{s.latest_version}</td>
                <td className="p-3">{s.field_count}</td>
                <td className="p-3 text-[var(--muted)]">{s.created_at}</td>
                <td className="p-3">
                  <div className="flex items-center justify-end gap-3">
                    <Link className="font-semibold text-[var(--accent)] hover:underline" to={`/schemas/${encodeURIComponent(s.name)}`}>
                      view
                    </Link>
                    <button
                      type="button"
                      onClick={() => onDelete(s.name)}
                      disabled={deleting === s.name}
                      className="cursor-pointer border-0 bg-transparent font-semibold text-[var(--fail)] hover:underline disabled:opacity-50"
                    >
                      {deleting === s.name ? "deleting…" : "delete"}
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </TableWrap>
      )}
    </div>
  );
}
