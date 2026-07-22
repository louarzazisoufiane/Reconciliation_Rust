import { useEffect, useState, type FormEvent } from "react";
import { api, ApiRequestError } from "./api";
import type { DeltaRow, Layout, LayoutField } from "./types";

const blank = (): LayoutField => ({ name: "", start: 1, end: 1, is_primary_key: false });
const currentLocalTimestamp = () => new Date().toISOString().slice(0, 19);
const toUtcTimestamp = (localTimestamp: string) => new Date(localTimestamp).toISOString();

export default function App() {
  const [layouts, setLayouts] = useState<Layout[]>([]);
  const [fields, setFields] = useState<LayoutField[]>([blank(), blank(), blank()]);
  const [layoutName, setLayoutName] = useState("");
  const [oldLayout, setOldLayout] = useState("");
  const [newLayout, setNewLayout] = useState("");
  const [oldFile, setOldFile] = useState<File | null>(null);
  const [newFile, setNewFile] = useState<File | null>(null);
  const [oldDate, setOldDate] = useState(currentLocalTimestamp);
  const [newDate, setNewDate] = useState(currentLocalTimestamp);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [delta, setDelta] = useState<DeltaRow[]>([]);

  const refresh = () => api.listLayouts().then((items) => { setLayouts(items); if (!oldLayout && items[0]) setOldLayout(items[0].id); if (!newLayout && items[0]) setNewLayout(items[0].id); }).catch((e) => setError(e.message));
  useEffect(() => { refresh(); }, []);

  function update(i: number, patch: Partial<LayoutField>) { setFields((old) => old.map((field, n) => n === i ? { ...field, ...patch } : field)); }
  async function saveLayout(e: FormEvent) {
    e.preventDefault(); setError(null); setMessage(null);
    try {
      const layout = await api.createLayout({ name: layoutName, fields: fields.filter((field) => field.name.trim()) });
      setLayouts((old) => [...old, layout].sort((a, b) => a.name.localeCompare(b.name))); setOldLayout(layout.id); setNewLayout(layout.id);
      setLayoutName(""); setFields([blank(), blank(), blank()]); setMessage(`Saved layout “${layout.name}”.`);
    } catch (e) { setError(e instanceof Error ? e.message : "Could not save layout"); }
  }
  async function compare(e: FormEvent) {
    e.preventDefault(); setError(null); setMessage(null); setDelta([]);
    if (!oldFile || !newFile || !oldLayout || !newLayout) { setError("Choose an old file, new file, and a layout for each."); return; }
    const data = new FormData();
    // Appending metadata first lets the server validate it before streaming each file.
    data.append("old_layout_id", oldLayout); data.append("new_layout_id", newLayout);
    data.append("old_date_of_download", toUtcTimestamp(oldDate)); data.append("new_date_of_download", toUtcTimestamp(newDate));
    // File names are load-level metadata. They are stored once on the
    // comparison run, never copied into every parsed source row.
    data.append("old_origin_file_name", oldFile.name); data.append("new_origin_file_name", newFile.name);
    data.append("old_file", oldFile); data.append("new_file", newFile);
    setRunning(true);
    try {
      const result = await api.createComparison(data);
      setMessage(`Loaded ${result.old_rows} old and ${result.new_rows} new rows — ${result.added} added, ${result.removed} removed, ${result.modified} modified.`);
      setDelta(await api.getDelta(result.id));
    } catch (e) { setError(e instanceof ApiRequestError || e instanceof Error ? e.message : "Comparison failed"); }
    finally { setRunning(false); }
  }

  return <div className="min-h-screen">
    <nav className="sticky top-0 z-10 border-b border-[var(--border)] bg-[var(--head)]/85 backdrop-blur-md"><div className="mx-auto flex max-w-5xl items-center gap-2 px-6 py-3 font-semibold"><span className="grid h-7 w-7 place-items-center rounded-lg bg-[var(--accent)] text-white">⇄</span><span className="font-display text-lg">Fixed-width Reconcile</span></div></nav>
    <main className="mx-auto max-w-5xl px-6 py-8">
      <header className="mb-7"><h1 className="font-display text-3xl font-semibold">Compare fixed-width files</h1><p className="mt-1 text-[var(--muted)]">Save reusable layouts, upload old and new files, then review their database-backed delta.</p></header>
      {error && <p className="mb-4 rounded-lg bg-[var(--fail-soft)] p-3 text-[var(--fail)]">{error}</p>}
      {message && <p className="mb-4 rounded-lg bg-[var(--pass-soft)] p-3 text-[var(--pass)]">{message}</p>}
      <section className="mb-8 rounded-xl border border-[var(--border)] bg-[var(--surface)] p-5 shadow-[var(--shadow-sm)]"><h2 className="font-display text-xl font-semibold">1. Create a layout</h2><p className="mb-4 text-sm text-[var(--muted)]">Positions are 1-based and inclusive. Select every field contributing to the composite primary key.</p>
        <form onSubmit={saveLayout}><input className="mb-3 w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" required value={layoutName} onChange={(e) => setLayoutName(e.target.value)} placeholder="Layout name, e.g. customers-v1" />
          <div className="grid grid-cols-[1fr_90px_90px_100px] gap-2 text-sm font-semibold text-[var(--muted)]"><span>Field name</span><span>Start</span><span>End</span><span>Primary key</span></div>
          {fields.map((field, i) => <div className="mt-2 grid grid-cols-[1fr_90px_90px_100px] gap-2" key={i}><input className="rounded-lg border border-[var(--border)] px-3 py-2" value={field.name} onChange={(e) => update(i, { name: e.target.value })} placeholder="customer_id"/><input className="rounded-lg border border-[var(--border)] px-3 py-2" type="number" min="1" value={field.start} onChange={(e) => update(i, { start: Number(e.target.value) })}/><input className="rounded-lg border border-[var(--border)] px-3 py-2" type="number" min="1" value={field.end} onChange={(e) => update(i, { end: Number(e.target.value) })}/><label className="flex items-center justify-center"><input type="checkbox" checked={field.is_primary_key} onChange={(e) => update(i, { is_primary_key: e.target.checked })}/></label></div>)}
          <div className="mt-4 flex gap-3"><button type="button" className="rounded-lg border border-[var(--border)] px-3 py-2" onClick={() => setFields((old) => [...old, blank()])}>+ Add field</button><button className="rounded-lg bg-[var(--accent)] px-3 py-2 font-semibold text-white">Save layout</button></div>
        </form>
      </section>
      <section className="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-5 shadow-[var(--shadow-sm)]"><h2 className="font-display text-xl font-semibold">2. Upload and compare</h2>
        <form onSubmit={compare} className="mt-4 grid gap-4 md:grid-cols-2">{(["old", "new"] as const).map((side) => <div key={side} className="rounded-lg bg-[var(--surface-2)] p-4"><h3 className="mb-3 font-semibold capitalize">{side} file</h3><label className="block text-sm font-medium">Layout<select className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" required value={side === "old" ? oldLayout : newLayout} onChange={(e) => side === "old" ? setOldLayout(e.target.value) : setNewLayout(e.target.value)}><option value="">Choose a saved layout…</option>{layouts.map((layout) => <option key={layout.id} value={layout.id}>{layout.name}</option>)}</select></label><label className="mt-3 block text-sm font-medium">Download time<input className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" type="datetime-local" step="1" required value={side === "old" ? oldDate : newDate} onChange={(e) => side === "old" ? setOldDate(e.target.value) : setNewDate(e.target.value)}/></label><label className="mt-3 block text-sm font-medium">Fixed-width file<input className="mt-1 block w-full text-sm" type="file" required onChange={(e) => side === "old" ? setOldFile(e.target.files?.[0] ?? null) : setNewFile(e.target.files?.[0] ?? null)}/></label></div>)}
          <div className="md:col-span-2"><button disabled={running} className="rounded-lg bg-[var(--accent)] px-4 py-2 font-semibold text-white disabled:opacity-60">{running ? "Loading and comparing…" : "Load files and compute delta"}</button></div>
        </form>
      </section>
      {delta.length > 0 && <section className="mt-8"><h2 className="font-display text-xl font-semibold">Delta</h2><div className="mt-3 overflow-auto rounded-xl border border-[var(--border)] bg-[var(--surface)]"><table className="w-full text-left text-sm"><thead className="bg-[var(--surface-2)] text-[var(--muted)]"><tr><th className="p-3">Key</th><th className="p-3">Change</th><th className="p-3">Values</th></tr></thead><tbody>{delta.map((row, i) => <tr className="border-t border-[var(--border)] align-top" key={i}><td className="p-3 font-mono text-xs">{row.composite_primary_key}</td><td className="p-3 font-semibold capitalize">{row.change_type}</td><td className="p-3"><ChangedValues row={row} /></td></tr>)}</tbody></table></div></section>}
    </main></div>;
}

function ChangedValues({ row }: { row: DeltaRow }) {
  const changed = Object.entries(row.changed_fields);
  const values = changed.length > 0
    ? changed.map(([field, value]) => ({ field, oldValue: value.old, newValue: value.new }))
    : Array.from(new Set([...Object.keys(row.old_data ?? {}), ...Object.keys(row.new_data ?? {})])).map((field) => ({ field, oldValue: row.old_data?.[field] ?? null, newValue: row.new_data?.[field] ?? null }));

  return <table className="min-w-[360px] border border-[var(--border)] text-xs"><thead className="bg-[var(--surface-2)] text-[var(--muted)]"><tr><th className="p-2 text-left">Field</th><th className="p-2 text-left">Old value</th><th className="p-2 text-left">New value</th></tr></thead><tbody>{values.map(({ field, oldValue, newValue }) => <tr className="border-t border-[var(--border)]" key={field}><td className="p-2 font-medium">{field}</td><td className="p-2">{oldValue ?? "—"}</td><td className="p-2">{newValue ?? "—"}</td></tr>)}</tbody></table>;
}
