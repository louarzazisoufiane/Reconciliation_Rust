import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Link } from "react-router-dom";
import { api, ApiRequestError } from "../api";
import type { Normalization, SchemaInfo } from "../types";
import { ErrorBanner } from "../components/Banner";
import { Button, Input, Label, TableWrap } from "../components/ui";

interface ColumnToggles extends Normalization {
  compare: boolean;
}

export default function BuildRun() {
  const [schemas, setSchemas] = useState<SchemaInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [step, setStep] = useState<1 | 2>(1);
  const [submitting, setSubmitting] = useState(false);
  const [success, setSuccess] = useState<{ reportUrl: string } | null>(null);

  // Step 1
  const [runName, setRunName] = useState("");
  const [pathA, setPathA] = useState("");
  const [pathB, setPathB] = useState("");
  const [schemaA, setSchemaA] = useState("");
  const [schemaB, setSchemaB] = useState("");

  // Step 2
  const [commonColumns, setCommonColumns] = useState<string[]>([]);
  const [key, setKey] = useState("");
  const [toggles, setToggles] = useState<Record<string, ColumnToggles>>({});

  useEffect(() => {
    api
      .listSchemas()
      .then(setSchemas)
      .catch((e) => setError(e.message));
  }, []);

  const schemaOptions = useMemo(() => {
    const opts: { value: string; label: string }[] = [];
    for (const s of schemas ?? []) {
      for (let v = 1; v <= s.latest_version; v++) {
        opts.push({ value: `${s.name}@${v}`, label: `${s.name} v${v}` });
      }
    }
    return opts;
  }, [schemas]);

  function parseRef(v: string) {
    const [name, version] = v.split("@");
    return { name, version: Number(version) };
  }

  async function onStep1Submit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    if (!schemaA || !schemaB) {
      setError("choose a schema for both sources");
      return;
    }
    try {
      const res = await api.validateRun({ schema_a: parseRef(schemaA), schema_b: parseRef(schemaB) });
      setCommonColumns(res.common_columns);
      setToggles(
        Object.fromEntries(
          res.common_columns.map((c) => [c, { compare: true, trim: true, strip_leading_zeros: false, unify_null: false, case_fold: false }]),
        ),
      );
      setKey(res.common_columns[0] ?? "");
      setStep(2);
    } catch (err) {
      setError(err instanceof ApiRequestError ? err.message : "failed to validate schemas");
    }
  }

  function updateToggle(col: string, patch: Partial<ColumnToggles>) {
    setToggles((prev) => ({ ...prev, [col]: { ...prev[col], ...patch } }));
  }

  async function onStep2Submit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    const compareColumns = commonColumns.filter((c) => toggles[c]?.compare);
    if (!key || compareColumns.length === 0) {
      setError("choose a key and at least one compare column");
      return;
    }
    const normalization: Record<string, Normalization> = {};
    for (const c of [key, ...compareColumns]) {
      const t = toggles[c];
      normalization[c] = {
        trim: t?.trim ?? true,
        strip_leading_zeros: t?.strip_leading_zeros ?? false,
        unify_null: t?.unify_null ?? false,
        case_fold: t?.case_fold ?? false,
      };
    }
    setSubmitting(true);
    try {
      const res = await api.createRun({
        run_name: runName,
        path_a: pathA,
        path_b: pathB,
        schema_a: parseRef(schemaA),
        schema_b: parseRef(schemaB),
        key,
        compare_columns: compareColumns,
        normalization,
      });
      setSuccess({ reportUrl: res.report_url });
    } catch (err) {
      setError(err instanceof ApiRequestError ? err.message : "failed to run reconciliation");
    } finally {
      setSubmitting(false);
    }
  }

  if (success) {
    return (
      <div>
        <h1 className="font-display text-2xl font-semibold text-[var(--fg)]">
          Run complete
        </h1>
        <p className="my-3 inline-flex items-center gap-2 rounded-lg bg-[var(--pass-soft)] px-3 py-2 font-semibold text-[var(--pass)]">
          ✓ Reconciliation finished.
        </p>
        <p className="flex gap-4">
          <a className="font-semibold text-[var(--accent)] hover:underline" href={success.reportUrl}>
            Open report →
          </a>
          <Link className="font-semibold text-[var(--accent)] hover:underline" to="/runs">
            View all runs
          </Link>
        </p>
      </div>
    );
  }

  return (
    <div>
      <header className="mb-5">
        <h1 className="font-display text-2xl font-semibold text-[var(--fg)]">
          Build a run
        </h1>
        <div className="text-sm text-[var(--muted)]">
          {step === 1 ? "Step 1 — sources & schemas" : `Step 2 — key, columns & normalization for "${runName}"`}
        </div>
      </header>
      {error && <ErrorBanner message={error} />}

      {step === 1 &&
        (schemas && schemas.length === 0 ? (
          <p className="text-[var(--muted)]">
            No schemas yet.{" "}
            <Link className="font-semibold text-[var(--accent)] hover:underline" to="/schemas/new">
              Create one first.
            </Link>
          </p>
        ) : (
          <form onSubmit={onStep1Submit} className="max-w-3xl">
            <Label htmlFor="run_name">Run name</Label>
            <Input
              id="run_name"
              required
              className="w-full"
              value={runName}
              onChange={(e) => setRunName(e.target.value)}
              placeholder="customers_daily_recon"
            />

            <Label htmlFor="path_a">Source A — file path</Label>
            <Input
              id="path_a"
              required
              className="w-full"
              value={pathA}
              onChange={(e) => setPathA(e.target.value)}
              placeholder="/landing/a/customers.txt"
            />
            <Label htmlFor="schema_a">Source A — schema</Label>
            <select
              id="schema_a"
              required
              className="min-w-[220px] rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-sm text-[var(--fg)] shadow-[var(--shadow-sm)]"
              value={schemaA}
              onChange={(e) => setSchemaA(e.target.value)}
            >
              <option value="" disabled>
                choose…
              </option>
              {schemaOptions.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>

            <Label htmlFor="path_b">Source B — file path</Label>
            <Input
              id="path_b"
              required
              className="w-full"
              value={pathB}
              onChange={(e) => setPathB(e.target.value)}
              placeholder="/landing/b/customers.txt"
            />
            <Label htmlFor="schema_b">Source B — schema</Label>
            <select
              id="schema_b"
              required
              className="min-w-[220px] rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-sm text-[var(--fg)] shadow-[var(--shadow-sm)]"
              value={schemaB}
              onChange={(e) => setSchemaB(e.target.value)}
            >
              <option value="" disabled>
                choose…
              </option>
              {schemaOptions.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>

            <p className="mt-4">
              <Button type="submit">Next: choose key & columns</Button>
            </p>
          </form>
        ))}

      {step === 2 && (
        <form onSubmit={onStep2Submit}>
          <p className="mb-3 text-xs text-[var(--muted)]">
            Only columns present (by name) in BOTH schemas are shown.
          </p>
          <TableWrap>
            <thead>
              <tr className="border-b border-[var(--border)] bg-[var(--surface-2)] text-center text-xs uppercase tracking-wide text-[var(--muted)]">
                <th className="p-3 text-left">Column</th>
                <th className="p-3">Key</th>
                <th className="p-3">Compare</th>
                <th className="p-3">trim</th>
                <th className="p-3">strip_leading_zeros</th>
                <th className="p-3">unify_null</th>
                <th className="p-3">case_fold</th>
              </tr>
            </thead>
            <tbody>
              {commonColumns.map((c) => (
                <tr key={c} className="border-b border-[var(--border)] text-center last:border-0">
                  <td className="p-3 text-left font-medium">{c}</td>
                  <td className="p-3">
                    <input type="radio" name="key" checked={key === c} onChange={() => setKey(c)} />
                  </td>
                  <td className="p-3">
                    <input
                      type="checkbox"
                      checked={toggles[c]?.compare ?? false}
                      onChange={(e) => updateToggle(c, { compare: e.target.checked })}
                    />
                  </td>
                  <td className="p-3">
                    <input
                      type="checkbox"
                      checked={toggles[c]?.trim ?? false}
                      onChange={(e) => updateToggle(c, { trim: e.target.checked })}
                    />
                  </td>
                  <td className="p-3">
                    <input
                      type="checkbox"
                      checked={toggles[c]?.strip_leading_zeros ?? false}
                      onChange={(e) => updateToggle(c, { strip_leading_zeros: e.target.checked })}
                    />
                  </td>
                  <td className="p-3">
                    <input
                      type="checkbox"
                      checked={toggles[c]?.unify_null ?? false}
                      onChange={(e) => updateToggle(c, { unify_null: e.target.checked })}
                    />
                  </td>
                  <td className="p-3">
                    <input
                      type="checkbox"
                      checked={toggles[c]?.case_fold ?? false}
                      onChange={(e) => updateToggle(c, { case_fold: e.target.checked })}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </TableWrap>
          <p className="mt-4 flex gap-3">
            <Button type="button" secondary onClick={() => setStep(1)}>
              ← back
            </Button>
            <Button type="submit" disabled={submitting}>
              {submitting ? "Running…" : "Run reconciliation"}
            </Button>
          </p>
        </form>
      )}
    </div>
  );
}
