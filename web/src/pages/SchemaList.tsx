import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import type { SchemaInfo } from "../types";
import { ErrorBanner } from "../components/Banner";
import { Button, TableWrap } from "../components/ui";

export default function SchemaList() {
  const [schemas, setSchemas] = useState<SchemaInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .listSchemas()
      .then(setSchemas)
      .catch((e) => setError(e.message));
  }, []);

  return (
    <div>
      <header className="mb-4">
        <h1 className="text-2xl font-bold">Schema library</h1>
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
            <tr className="border-b border-[var(--border)] text-left">
              <th className="p-2">Name</th>
              <th className="p-2">Latest</th>
              <th className="p-2">Fields</th>
              <th className="p-2">Created</th>
              <th className="p-2"></th>
            </tr>
          </thead>
          <tbody>
            {schemas.map((s) => (
              <tr key={s.name} className="border-b border-[var(--border)] last:border-0">
                <td className="p-2">{s.name}</td>
                <td className="p-2">v{s.latest_version}</td>
                <td className="p-2">{s.field_count}</td>
                <td className="p-2 text-[var(--muted)]">{s.created_at}</td>
                <td className="p-2">
                  <Link className="text-[var(--accent)]" to={`/schemas/${encodeURIComponent(s.name)}`}>
                    view
                  </Link>
                </td>
              </tr>
            ))}
          </tbody>
        </TableWrap>
      )}
    </div>
  );
}
