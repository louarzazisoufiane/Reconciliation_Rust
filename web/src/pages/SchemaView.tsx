import { useEffect, useState } from "react";
import { Link, useLocation, useParams, useSearchParams } from "react-router-dom";
import { api } from "../api";
import type { Schema } from "../types";
import { ErrorBanner, WarnBanner } from "../components/Banner";
import { TableWrap } from "../components/ui";

export default function SchemaView() {
  const { name = "" } = useParams();
  const [params] = useSearchParams();
  const version = params.get("v");
  const location = useLocation();
  const warnings = (location.state as { warnings?: string[] } | null)?.warnings ?? [];
  const [schema, setSchema] = useState<Schema | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setSchema(null);
    setError(null);
    api
      .getSchema(name, version ? Number(version) : undefined)
      .then(setSchema)
      .catch((e) => setError(e.message));
  }, [name, version]);

  if (error) {
    return (
      <div>
        <h1 className="text-2xl font-bold">{name}</h1>
        <ErrorBanner message={error} />
      </div>
    );
  }
  if (!schema) return <p className="text-[var(--muted)]">Loading…</p>;

  return (
    <div>
      <header className="mb-4">
        <h1 className="text-2xl font-bold">
          {schema.name} <span className="text-base font-normal text-[var(--muted)]">v{schema.version}</span>
        </h1>
        <div className="text-sm text-[var(--muted)]">
          encoding {schema.encoding} · index_base {schema.index_base}
        </div>
      </header>
      {warnings.length > 0 && <WarnBanner message={warnings.join("; ")} />}
      <TableWrap>
        <thead>
          <tr className="border-b border-[var(--border)] text-left">
            <th className="p-2">Field</th>
            <th className="p-2">Start</th>
            <th className="p-2">Length</th>
          </tr>
        </thead>
        <tbody>
          {schema.fields.map((f) => (
            <tr key={f.name} className="border-b border-[var(--border)] last:border-0">
              <td className="p-2">{f.name}</td>
              <td className="p-2">{f.start}</td>
              <td className="p-2">{f.length}</td>
            </tr>
          ))}
        </tbody>
      </TableWrap>
      <p className="mt-4 flex gap-3 text-sm">
        <Link className="text-[var(--accent)]" to={`/schemas/${encodeURIComponent(schema.name)}/edit`}>
          ✎ Edit
        </Link>
        <Link className="text-[var(--accent)]" to="/schemas">
          ← back to library
        </Link>
      </p>
    </div>
  );
}
