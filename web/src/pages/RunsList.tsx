import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api";
import type { ManifestEntry } from "../types";
import { ErrorBanner } from "../components/Banner";
import { TableWrap } from "../components/ui";

export default function RunsList() {
  const [entries, setEntries] = useState<ManifestEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .listRuns()
      .then(setEntries)
      .catch((e) => setError(e.message));
  }, []);

  return (
    <div>
      <header className="mb-4">
        <h1 className="text-2xl font-bold">Runs</h1>
        <div className="text-sm text-[var(--muted)]">{entries?.length ?? "…"} run(s)</div>
      </header>
      {error && <ErrorBanner message={error} />}
      {entries && entries.length === 0 && (
        <p className="text-[var(--muted)]">
          No runs yet.{" "}
          <Link className="text-[var(--accent)]" to="/runs/new">
            Build one.
          </Link>
        </p>
      )}
      {entries && entries.length > 0 && (
        <TableWrap>
          <thead>
            <tr className="border-b border-[var(--border)] text-left">
              <th className="p-2">When</th>
              <th className="p-2">Run</th>
              <th className="p-2">Result</th>
              <th className="p-2">Changed</th>
              <th className="p-2">Only A</th>
              <th className="p-2">Only B</th>
              <th className="p-2">Report</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => (
              <tr key={e.run_id} className="border-b border-[var(--border)] last:border-0">
                <td className="p-2 text-[var(--muted)]">{e.timestamp}</td>
                <td className="p-2">{e.run_name}</td>
                <td className="p-2">
                  {e.pass ? (
                    <span className="rounded bg-[var(--pass)] px-2 py-0.5 text-xs font-bold text-white">PASS</span>
                  ) : (
                    <span className="rounded bg-[var(--fail)] px-2 py-0.5 text-xs font-bold text-white">DIFF</span>
                  )}
                </td>
                <td className="p-2">{e.changed}</td>
                <td className="p-2">{e.only_in_a}</td>
                <td className="p-2">{e.only_in_b}</td>
                <td className="p-2">
                  <a className="text-[var(--accent)]" href={`/reports/${e.report_html}`}>
                    open
                  </a>
                </td>
              </tr>
            ))}
          </tbody>
        </TableWrap>
      )}
    </div>
  );
}
