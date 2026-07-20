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
      <header className="mb-5">
        <h1 className="font-display text-2xl font-semibold text-[var(--fg)]">
          Runs
        </h1>
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
            <tr className="border-b border-[var(--border)] bg-[var(--surface-2)] text-left text-xs uppercase tracking-wide text-[var(--muted)]">
              <th className="p-3">When</th>
              <th className="p-3">Run</th>
              <th className="p-3">Result</th>
              <th className="p-3">Changed</th>
              <th className="p-3">Only A</th>
              <th className="p-3">Only B</th>
              <th className="p-3">Report</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => (
              <tr key={e.run_id} className="border-b border-[var(--border)] last:border-0">
                <td className="p-3 text-[var(--muted)]">{e.timestamp}</td>
                <td className="p-3 font-medium">{e.run_name}</td>
                <td className="p-3">
                  {e.pass ? (
                    <span className="inline-flex items-center rounded-full bg-[var(--pass-soft)] px-2.5 py-0.5 text-xs font-bold text-[var(--pass)]">
                      ● PASS
                    </span>
                  ) : (
                    <span className="inline-flex items-center rounded-full bg-[var(--fail-soft)] px-2.5 py-0.5 text-xs font-bold text-[var(--fail)]">
                      ● DIFF
                    </span>
                  )}
                </td>
                <td className="p-3">{e.changed}</td>
                <td className="p-3">{e.only_in_a}</td>
                <td className="p-3">{e.only_in_b}</td>
                <td className="p-3">
                  <a className="font-semibold text-[var(--accent)] hover:underline" href={`/reports/${e.report_html}`}>
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
