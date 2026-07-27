import { useEffect, useState, type FormEvent } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { api, ApiRequestError } from "./api";
import type { ComparisonRun, DeltaRow, Layout, LayoutField, NewScheduledTask, ScheduledTask, ScheduleFrequency } from "./types";

const blank = (): LayoutField => ({ name: "", start: 1, end: 1, is_primary_key: false });
const currentLocalTimestamp = () => new Date().toISOString().slice(0, 19);
const toUtcTimestamp = (localTimestamp: string) => new Date(localTimestamp).toISOString();

function inferFieldsFromHeader(header: string): LayoutField[] {
  const line = header.split(/\r?\n/, 1)[0] ?? "";
  const fields: LayoutField[] = [];
  let segmentStart = 0;

  const addField = (segmentEnd: number, end?: number) => {
    const segment = line.slice(segmentStart, segmentEnd);
    const firstCharacter = segment.search(/\S/);
    if (firstCharacter === -1) return;

    const lastCharacter = segment.search(/\s*$/);
    fields.push({
      name: segment.slice(firstCharacter, lastCharacter),
      start: segmentStart + firstCharacter + 1,
      end: end ?? segmentStart + lastCharacter,
      is_primary_key: false,
    });
  };

  for (const separator of line.matchAll(/\s{1,}/g)) {
    // Separator padding is part of the preceding fixed-width column, so its
    // end reaches the character immediately before the next field starts.
    addField(separator.index, separator.index + separator[0].length);
    segmentStart = separator.index + separator[0].length;
  }
  addField(line.length);
  return fields;
}
type Page = "compare" | "history" | "detail" | "scheduled";

export default function App() {
  const location = useLocation();
  const navigate = useNavigate();
  const [layouts, setLayouts] = useState<Layout[]>([]);
  const [fields, setFields] = useState<LayoutField[]>([blank(), blank(), blank()]);
  const [layoutName, setLayoutName] = useState("");
  const [headerLine, setHeaderLine] = useState("");
  const [oldLayout, setOldLayout] = useState("");
  const [newLayout, setNewLayout] = useState("");
  const [oldFile, setOldFile] = useState<File | null>(null);
  const [newFile, setNewFile] = useState<File | null>(null);
  const [oldDate, setOldDate] = useState(currentLocalTimestamp);
  const [newDate, setNewDate] = useState(currentLocalTimestamp);
  const [runName, setRunName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [delta, setDelta] = useState<DeltaRow[]>([]);
  const [history, setHistory] = useState<ComparisonRun[]>([]);
  const [selectedRun, setSelectedRun] = useState<ComparisonRun | null>(null);
  const [selectedDelta, setSelectedDelta] = useState<DeltaRow[]>([]);
  const [page, setPage] = useState<Page>("compare");

  const refreshLayouts = () => api.listLayouts().then((items) => {
    setLayouts(items);
    if (!oldLayout && items[0]) setOldLayout(items[0].id);
    if (!newLayout && items[0]) setNewLayout(items[0].id);
  }).catch((e) => setError(e.message));
  useEffect(() => { refreshLayouts(); }, []);

  async function showHistory() {
    setError(null); setMessage(null); setPage("history"); navigate("/history");
    try { setHistory(await api.listComparisons()); }
    catch (e) { setError(e instanceof Error ? e.message : "Could not load run history"); }
  }
  function showScheduled() { setError(null); setMessage(null); setPage("scheduled"); navigate("/scheduled"); }
  function showComparison() { setError(null); setMessage(null); setPage("compare"); navigate("/"); }
  useEffect(() => { if (location.pathname === "/scheduled") setPage("scheduled"); else if (location.pathname === "/history") setPage("history"); else if (location.pathname === "/") setPage("compare"); }, [location.pathname]);
  async function openRun(run: ComparisonRun) {
    setError(null); setSelectedRun(run); setSelectedDelta([]); setPage("detail");
    try { setSelectedDelta(await api.getDelta(run.id)); }
    catch (e) { setError(e instanceof Error ? e.message : "Could not load this run"); }
  }
  function update(i: number, patch: Partial<LayoutField>) { setFields((old) => old.map((field, n) => n === i ? { ...field, ...patch } : field)); }
  async function saveLayout(e: FormEvent) {
    e.preventDefault(); setError(null); setMessage(null);
    try {
      const layout = await api.createLayout({ name: layoutName, fields: fields.filter((field) => field.name.trim()) });
      setLayouts((old) => [...old, layout].sort((a, b) => a.name.localeCompare(b.name))); setOldLayout(layout.id); setNewLayout(layout.id);
      setLayoutName(""); setHeaderLine(""); setFields([blank(), blank(), blank()]); setMessage(`Saved layout “${layout.name}”.`);
    } catch (e) { setError(e instanceof Error ? e.message : "Could not save layout"); }
  }
  async function compare(e: FormEvent) {
    e.preventDefault(); setError(null); setMessage(null); setDelta([]);
    if (!oldFile || !newFile || !oldLayout || !newLayout || !runName.trim()) { setError("Enter a run name, choose both files, and choose a layout for each."); return; }
    const data = new FormData();
    data.append("run_name", runName);
    data.append("processing_started_at", new Date().toISOString());
    data.append("old_layout_id", oldLayout); data.append("new_layout_id", newLayout);
    data.append("old_date_of_download", toUtcTimestamp(oldDate)); data.append("new_date_of_download", toUtcTimestamp(newDate));
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
    <nav className="sticky top-0 z-10 border-b border-[var(--border)] bg-[var(--head)]/85 backdrop-blur-md"><div className="mx-auto flex max-w-5xl items-center gap-4 px-6 py-3 font-semibold"><span className="grid h-7 w-7 place-items-center rounded-lg bg-[var(--accent)] text-white">⇄</span><span className="mr-auto font-display text-lg">Fixed-width Reconcile</span><button className={navClass(page === "compare")} onClick={showComparison}>New comparison</button><button className={navClass(page === "scheduled")} onClick={showScheduled}>Scheduled</button><button className={navClass(page === "history" || page === "detail")} onClick={showHistory}>History</button></div></nav>
    <main className="mx-auto max-w-5xl px-6 py-8">
      {error && <p className="mb-4 rounded-lg bg-[var(--fail-soft)] p-3 text-[var(--fail)]">{error}</p>}
      {message && <p className="mb-4 rounded-lg bg-[var(--pass-soft)] p-3 text-[var(--pass)]">{message}</p>}
      {page === "compare" && <>
        <header className="mb-7"><h1 className="font-display text-3xl font-semibold">Compare fixed-width files</h1><p className="mt-1 text-[var(--muted)]">Save reusable layouts, upload old and new files, then review their database-backed delta.</p></header>
        <section className="mb-8 rounded-xl border border-[var(--border)] bg-[var(--surface)] p-5 shadow-[var(--shadow-sm)]"><h2 className="font-display text-xl font-semibold">1. Create a layout</h2><p className="mb-4 text-sm text-[var(--muted)]">Positions are 1-based and inclusive. Select every field contributing to the composite primary key.</p>
          <form onSubmit={saveLayout}><input className="mb-3 w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" required value={layoutName} onChange={(e) => setLayoutName(e.target.value)} placeholder="Layout name, e.g. customers-v1" />
            <div className="mb-4"><label className="block text-sm font-medium">Header line<textarea className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2 font-mono text-sm" rows={2} value={headerLine} onChange={(e) => setHeaderLine(e.target.value)} placeholder="Customer ID    Full Name          Account     Balance" /></label><div className="mt-2 flex items-center gap-3"><button type="button" className="rounded-lg border border-[var(--border)] px-3 py-2" onClick={() => setFields(inferFieldsFromHeader(headerLine))}>Infer</button><span className="text-sm text-[var(--muted)]">Two or more spaces separate columns; single spaces stay in field names.</span></div></div>
            <div className="grid grid-cols-[1fr_90px_90px_100px] gap-2 text-sm font-semibold text-[var(--muted)]"><span>Field name</span><span>Start</span><span>End</span><span>Primary key</span></div>
            {fields.map((field, i) => <div className="mt-2 grid grid-cols-[1fr_90px_90px_100px] gap-2" key={i}><input className="rounded-lg border border-[var(--border)] px-3 py-2" value={field.name} onChange={(e) => update(i, { name: e.target.value })} placeholder="customer_id"/><input className="rounded-lg border border-[var(--border)] px-3 py-2" type="number" min="1" value={field.start} onChange={(e) => update(i, { start: Number(e.target.value) })}/><input className="rounded-lg border border-[var(--border)] px-3 py-2" type="number" min="1" value={field.end} onChange={(e) => update(i, { end: Number(e.target.value) })}/><label className="flex items-center justify-center"><input type="checkbox" checked={field.is_primary_key} onChange={(e) => update(i, { is_primary_key: e.target.checked })}/></label></div>)}
            <div className="mt-4 flex gap-3"><button type="button" className="rounded-lg border border-[var(--border)] px-3 py-2" onClick={() => setFields((old) => [...old, blank()])}>+ Add field</button><button className="rounded-lg bg-[var(--accent)] px-3 py-2 font-semibold text-white">Save layout</button></div>
          </form>
        </section>
        <section className="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-5 shadow-[var(--shadow-sm)]"><h2 className="font-display text-xl font-semibold">2. Upload and compare</h2>
          <form onSubmit={compare} className="mt-4 grid gap-4 md:grid-cols-2"><label className="md:col-span-2 block text-sm font-medium">Run name<input className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" required value={runName} onChange={(e) => setRunName(e.target.value)} placeholder="e.g. July customer refresh" /></label>{(["old", "new"] as const).map((side) => <div key={side} className="rounded-lg bg-[var(--surface-2)] p-4"><h3 className="mb-3 font-semibold capitalize">{side} file</h3><label className="block text-sm font-medium">Layout<select className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" required value={side === "old" ? oldLayout : newLayout} onChange={(e) => side === "old" ? setOldLayout(e.target.value) : setNewLayout(e.target.value)}><option value="">Choose a saved layout…</option>{layouts.map((layout) => <option key={layout.id} value={layout.id}>{layout.name}</option>)}</select></label><label className="mt-3 block text-sm font-medium">Download time<input className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" type="datetime-local" step="1" required value={side === "old" ? oldDate : newDate} onChange={(e) => side === "old" ? setOldDate(e.target.value) : setNewDate(e.target.value)}/></label><label className="mt-3 block text-sm font-medium">Fixed-width file<input className="mt-1 block w-full text-sm" type="file" required onChange={(e) => side === "old" ? setOldFile(e.target.files?.[0] ?? null) : setNewFile(e.target.files?.[0] ?? null)}/></label></div>)}<div className="md:col-span-2"><button disabled={running} className="rounded-lg bg-[var(--accent)] px-4 py-2 font-semibold text-white disabled:opacity-60">{running ? "Loading and comparing…" : "Load files and compute delta"}</button></div></form>
        </section>
        {delta.length > 0 && <DeltaTable title="Delta" delta={delta}/>}
      </>}
      {page === "history" && <History runs={history} onOpen={openRun}/>}
      {page === "detail" && selectedRun && <><button className="mb-5 rounded-lg border border-[var(--border)] px-3 py-2" onClick={showHistory}>← Back to history</button><RunSummary run={selectedRun}/><DeltaTable title="Stored differences" delta={selectedDelta}/></>}
      {page === "scheduled" && <ScheduledPage layouts={layouts} onError={setError} onMessage={setMessage}/>}
    </main>
  </div>;
}

const blankSchedule = (): NewScheduledTask => ({ name: "", frequency: "one_time", run_at: currentLocalTimestamp(), old_path: "", new_path: "", archive_path: "", old_layout_id: "", new_layout_id: "" });
function frequencyLabel(value: ScheduleFrequency) { return ({ one_time: "One Time", daily: "Daily", weekly: "Weekly", monthly: "Monthly" })[value]; }
function ScheduleForm({ layouts, value, submitLabel, onSubmit, onCancel }: { layouts: Layout[]; value: NewScheduledTask; submitLabel: string; onSubmit: (value: NewScheduledTask) => Promise<void>; onCancel?: () => void }) {
  const [draft, setDraft] = useState(value); const [saving, setSaving] = useState(false);
  useEffect(() => setDraft(value), [value]);
  useEffect(() => setDraft((old) => ({ ...old, old_layout_id: old.old_layout_id || layouts[0]?.id || "", new_layout_id: old.new_layout_id || layouts[0]?.id || "" })), [layouts]);
  const set = <K extends keyof NewScheduledTask>(key: K, next: NewScheduledTask[K]) => setDraft((old) => ({ ...old, [key]: next }));
  async function submit(event: FormEvent) { event.preventDefault(); setSaving(true); try { await onSubmit({ ...draft, run_at: toUtcTimestamp(draft.run_at) }); } finally { setSaving(false); } }
  return <form onSubmit={submit} className="grid gap-3 md:grid-cols-2"><label className="block text-sm font-medium md:col-span-2">Name<input required className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" value={draft.name} onChange={(e) => set("name", e.target.value)} placeholder="e.g. nightly customer files"/></label><label className="block text-sm font-medium">Frequency<select className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" value={draft.frequency} onChange={(e) => set("frequency", e.target.value as ScheduleFrequency)}>{(["one_time", "daily", "weekly", "monthly"] as ScheduleFrequency[]).map((item) => <option key={item} value={item}>{frequencyLabel(item)}</option>)}</select></label><label className="block text-sm font-medium">First run at<input required type="datetime-local" step="1" className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" value={draft.run_at} onChange={(e) => set("run_at", e.target.value)}/></label><label className="block text-sm font-medium">Old path<input required className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" value={draft.old_path} onChange={(e) => set("old_path", e.target.value)} placeholder="/srv/reconciliation/old"/></label><label className="block text-sm font-medium">New path<input required className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" value={draft.new_path} onChange={(e) => set("new_path", e.target.value)} placeholder="/srv/reconciliation/new"/></label><label className="block text-sm font-medium md:col-span-2">Archive path<input required className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" value={draft.archive_path} onChange={(e) => set("archive_path", e.target.value)} placeholder="/srv/reconciliation/archive"/></label><label className="block text-sm font-medium">Old layout<select required className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" value={draft.old_layout_id} onChange={(e) => set("old_layout_id", e.target.value)}><option value="">Choose layout…</option>{layouts.map((layout) => <option key={layout.id} value={layout.id}>{layout.name}</option>)}</select></label><label className="block text-sm font-medium">New layout<select required className="mt-1 block w-full rounded-lg border border-[var(--border)] bg-white px-3 py-2" value={draft.new_layout_id} onChange={(e) => set("new_layout_id", e.target.value)}><option value="">Choose layout…</option>{layouts.map((layout) => <option key={layout.id} value={layout.id}>{layout.name}</option>)}</select></label><div className="flex gap-3 md:col-span-2"><button disabled={saving} className="rounded-lg bg-[var(--accent)] px-4 py-2 font-semibold text-white disabled:opacity-60">{saving ? "Saving…" : submitLabel}</button>{onCancel && <button type="button" className="rounded-lg border border-[var(--border)] px-4 py-2" onClick={onCancel}>Cancel edit</button>}</div></form>;
}
function ScheduledPage({ layouts, onError, onMessage }: { layouts: Layout[]; onError: (value: string | null) => void; onMessage: (value: string | null) => void }) {
  const [tasks, setTasks] = useState<ScheduledTask[]>([]); const [draft, setDraft] = useState(blankSchedule); const [editing, setEditing] = useState<ScheduledTask | null>(null); const [all, setAll] = useState(false); const [loading, setLoading] = useState(true);
  async function refresh() { setLoading(true); try { setTasks(await api.listScheduled(all ? "all" : "pending")); } catch (e) { onError(e instanceof Error ? e.message : "Could not load schedules"); } finally { setLoading(false); } }
  useEffect(() => { refresh(); }, [all]);
  async function create(value: NewScheduledTask) { onError(null); try { await api.createScheduled(value); setDraft(blankSchedule()); onMessage("Scheduled task created."); await refresh(); } catch (e) { onError(e instanceof Error ? e.message : "Could not create schedule"); } }
  async function update(value: NewScheduledTask) { if (!editing) return; onError(null); try { await api.updateScheduled(editing.id, value); setEditing(null); onMessage("Scheduled task updated."); await refresh(); } catch (e) { onError(e instanceof Error ? e.message : "Could not update schedule"); } }
  async function remove(task: ScheduledTask) { if (!window.confirm(`Delete scheduled task “${task.name}”?`)) return; try { await api.deleteScheduled(task.id); onMessage("Scheduled task deleted."); await refresh(); } catch (e) { onError(e instanceof Error ? e.message : "Could not delete schedule"); } }
  async function runNow(task: ScheduledTask) { try { await api.runScheduledNow(task.id); onMessage(`“${task.name}” is running.`); await refresh(); } catch (e) { onError(e instanceof Error ? e.message : "Could not start schedule"); } }
  const editValue = editing && { name: editing.name, frequency: editing.frequency, run_at: new Date(editing.run_at).toISOString().slice(0, 19), old_path: editing.old_path, new_path: editing.new_path, archive_path: editing.archive_path, old_layout_id: editing.old_layout_id, new_layout_id: editing.new_layout_id };
  return <><header className="mb-7"><h1 className="font-display text-3xl font-semibold">Scheduled comparisons</h1><p className="mt-1 text-[var(--muted)]">Run directory-based comparisons once or on a recurring cadence.</p></header><section className="mb-8 rounded-xl border border-[var(--border)] bg-[var(--surface)] p-5 shadow-[var(--shadow-sm)]"><h2 className="mb-4 font-display text-xl font-semibold">{editing ? "Edit scheduled task" : "Create scheduled task"}</h2><ScheduleForm layouts={layouts} value={editValue ?? draft} submitLabel={editing ? "Save schedule" : "Create schedule"} onSubmit={editing ? update : create} onCancel={editing ? () => setEditing(null) : undefined}/></section><section className="rounded-xl border border-[var(--border)] bg-[var(--surface)] shadow-[var(--shadow-sm)]"><div className="flex items-center justify-between gap-3 p-5"><div><h2 className="font-display text-xl font-semibold">Pending schedules</h2><p className="text-sm text-[var(--muted)]">Tasks waiting to run or needing attention.</p></div><label className="text-sm"><input className="mr-2" type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)}/>Show completed</label></div>{loading ? <p className="border-t border-[var(--border)] p-5 text-[var(--muted)]">Loading schedules…</p> : tasks.length === 0 ? <p className="border-t border-[var(--border)] p-5 text-[var(--muted)]">No scheduled tasks to show.</p> : <div className="overflow-auto border-t border-[var(--border)]"><table className="w-full text-left text-sm"><thead className="bg-[var(--surface-2)] text-[var(--muted)]"><tr><th className="p-3">Name</th><th className="p-3">Frequency</th><th className="p-3">Next Run At</th><th className="p-3">Status</th><th className="p-3">Last Run At</th><th className="p-3">Actions</th></tr></thead><tbody>{tasks.map((task) => <tr className="border-t border-[var(--border)]" key={task.id}><td className="p-3 font-medium">{task.name}{task.error_message && <div className="mt-1 max-w-xs text-xs text-[var(--fail)]">{task.error_message}</div>}</td><td className="p-3">{frequencyLabel(task.frequency)}</td><td className="p-3 whitespace-nowrap">{formatTime(task.run_at)}</td><td className="p-3"><StatusBadge status={task.status}/></td><td className="p-3 whitespace-nowrap">{formatTime(task.last_run_at)}</td><td className="p-3 whitespace-nowrap"><button className="mr-2 rounded border border-[var(--border)] px-2 py-1" onClick={() => setEditing(task)}>Edit</button><button className="mr-2 rounded border border-[var(--border)] px-2 py-1 text-[var(--fail)]" onClick={() => remove(task)}>Delete</button><button disabled={task.status === "running" || task.status === "completed"} className="rounded bg-[var(--accent)] px-2 py-1 text-white disabled:opacity-50" onClick={() => runNow(task)}>Run now</button></td></tr>)}</tbody></table></div>}</section></>;
}
function StatusBadge({ status }: { status: ScheduledTask["status"] }) { const style = status === "failed" ? "bg-[var(--fail-soft)] text-[var(--fail)]" : status === "completed" ? "bg-[var(--pass-soft)] text-[var(--pass)]" : status === "running" ? "bg-[var(--accent-soft)] text-[var(--accent)]" : "bg-[var(--surface-2)] text-[var(--muted)]"; return <span className={`rounded-full px-2 py-1 text-xs font-semibold capitalize ${style}`}>{status}</span>; }

function navClass(active: boolean) { return `rounded-lg px-3 py-1.5 text-sm ${active ? "bg-[var(--accent)] text-white" : "text-[var(--muted)]"}`; }
function formatTime(value: string | null) { return value ? new Date(value).toLocaleString() : "—"; }
function formatDuration(milliseconds: number | null) { if (milliseconds === null) return "—"; const totalSeconds = Math.floor(milliseconds / 1000); const hours = Math.floor(totalSeconds / 3600); const minutes = Math.floor((totalSeconds % 3600) / 60); const seconds = totalSeconds % 60; return hours > 0 ? `${hours}h ${minutes}m ${seconds}s` : `${minutes}m ${seconds}s`; }
function RunSummary({ run }: { run: ComparisonRun }) { return <header className="mb-6"><h1 className="font-display text-3xl font-semibold">{run.run_name}</h1><p className="mt-1 text-[var(--muted)]">Run index {run.run_index} · Created {formatTime(run.created_at)} · Processing time {formatDuration(run.processing_duration_ms)}</p><div className="mt-4 grid gap-3 md:grid-cols-2"><SourceCard label="Old" file={run.old_origin_file_name} layout={run.old_layout_name} date={run.old_date_of_download}/><SourceCard label="New" file={run.new_origin_file_name} layout={run.new_layout_name} date={run.new_date_of_download}/></div></header>; }
function SourceCard({ label, file, layout, date }: { label: string; file: string | null; layout: string; date: string | null }) { return <div className="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-4"><h2 className="font-semibold">{label} file</h2><dl className="mt-2 space-y-1 text-sm"><div><dt className="inline text-[var(--muted)]">File: </dt><dd className="inline">{file ?? "—"}</dd></div><div><dt className="inline text-[var(--muted)]">Layout: </dt><dd className="inline">{layout}</dd></div><div><dt className="inline text-[var(--muted)]">Download: </dt><dd className="inline">{formatTime(date)}</dd></div></dl></div>; }
function History({ runs, onOpen }: { runs: ComparisonRun[]; onOpen: (run: ComparisonRun) => void }) { return <><header className="mb-6"><h1 className="font-display text-3xl font-semibold">Run history</h1><p className="mt-1 text-[var(--muted)]">Open any completed comparison to review its stored differences.</p></header>{runs.length === 0 ? <p className="rounded-xl border border-[var(--border)] bg-[var(--surface)] p-5 text-[var(--muted)]">No comparison runs yet.</p> : <div className="overflow-auto rounded-xl border border-[var(--border)] bg-[var(--surface)]"><table className="w-full text-left text-sm"><thead className="bg-[var(--surface-2)] text-[var(--muted)]"><tr><th className="p-3">Run</th><th className="p-3">Run name</th><th className="p-3">Created</th><th className="p-3">Processing time</th><th className="p-3">Old file / layout</th><th className="p-3">New file / layout</th><th className="p-3"></th></tr></thead><tbody>{runs.map((run) => <tr className="border-t border-[var(--border)]" key={run.id}><td className="p-3 font-mono">{run.run_index}</td><td className="p-3 font-medium">{run.run_name}</td><td className="p-3 whitespace-nowrap">{formatTime(run.created_at)}</td><td className="p-3 whitespace-nowrap">{formatDuration(run.processing_duration_ms)}</td><td className="p-3"><div>{run.old_origin_file_name ?? "—"}</div><div className="text-xs text-[var(--muted)]">{run.old_layout_name}</div></td><td className="p-3"><div>{run.new_origin_file_name ?? "—"}</div><div className="text-xs text-[var(--muted)]">{run.new_layout_name}</div></td><td className="p-3"><button className="rounded-lg bg-[var(--accent)] px-3 py-2 font-semibold text-white" onClick={() => onOpen(run)}>View differences</button></td></tr>)}</tbody></table></div>}</>}
function DeltaTable({ title, delta }: { title: string; delta: DeltaRow[] }) { return <section className="mt-8"><h2 className="font-display text-xl font-semibold">{title}</h2>{delta.length === 0 ? <p className="mt-3 rounded-xl border border-[var(--border)] bg-[var(--surface)] p-4 text-[var(--muted)]">No differences were recorded for this run.</p> : <div className="mt-3 overflow-auto rounded-xl border border-[var(--border)] bg-[var(--surface)]"><table className="w-full text-left text-sm"><thead className="bg-[var(--surface-2)] text-[var(--muted)]"><tr><th className="p-3">Key</th><th className="p-3">Change</th><th className="p-3">Values</th></tr></thead><tbody>{delta.map((row, i) => <tr className="border-t border-[var(--border)] align-top" key={i}><td className="p-3 font-mono text-xs">{row.composite_primary_key}</td><td className="p-3 font-semibold capitalize">{row.change_type}</td><td className="p-3"><ChangedValues row={row}/></td></tr>)}</tbody></table></div>}</section>; }
function ChangedValues({ row }: { row: DeltaRow }) { const changed = Object.entries(row.changed_fields); const values = changed.length > 0 ? changed.map(([field, value]) => ({ field, oldValue: value.old, newValue: value.new })) : Array.from(new Set([...Object.keys(row.old_data ?? {}), ...Object.keys(row.new_data ?? {})])).map((field) => ({ field, oldValue: row.old_data?.[field] ?? null, newValue: row.new_data?.[field] ?? null })); return <table className="min-w-[360px] border border-[var(--border)] text-xs"><thead className="bg-[var(--surface-2)] text-[var(--muted)]"><tr><th className="p-2 text-left">Field</th><th className="p-2 text-left">Old value</th><th className="p-2 text-left">New value</th></tr></thead><tbody>{values.map(({ field, oldValue, newValue }) => <tr className="border-t border-[var(--border)]" key={field}><td className="p-2 font-medium">{field}</td><td className="p-2">{oldValue ?? "—"}</td><td className="p-2">{newValue ?? "—"}</td></tr>)}</tbody></table>; }
