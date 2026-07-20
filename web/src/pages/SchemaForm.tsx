import { useEffect, useState, type FormEvent } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { api, ApiRequestError } from "../api";
import type { Field } from "../types";
import { ErrorBanner, WarnBanner } from "../components/Banner";
import { Button, Input, Label } from "../components/ui";

const emptyRow: Field = { name: "", start: 0, length: 0 };

export default function SchemaForm({ mode }: { mode: "create" | "edit" }) {
  const { name: editName } = useParams();
  const navigate = useNavigate();

  const [name, setName] = useState("");
  const [encoding, setEncoding] = useState("utf-8");
  const [indexBase, setIndexBase] = useState(0);
  const [fields, setFields] = useState<Field[]>([{ ...emptyRow }, { ...emptyRow }, { ...emptyRow }, { ...emptyRow }]);

  const [sample, setSample] = useState("");
  const [draftNote, setDraftNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (mode === "edit" && editName) {
      api
        .getSchema(editName)
        .then((s) => {
          setName(s.name);
          setEncoding(s.encoding);
          setIndexBase(s.index_base);
          setFields(s.fields);
        })
        .catch((e) => setError(e.message));
    }
  }, [mode, editName]);

  function updateField(i: number, patch: Partial<Field>) {
    setFields((prev) => prev.map((f, idx) => (idx === i ? { ...f, ...patch } : f)));
  }

  function addRow() {
    setFields((prev) => [...prev, { ...emptyRow }]);
  }

  async function inferDraft() {
    setError(null);
    const draft = await api.inferSchema(sample).catch((e) => {
      setError(e.message);
      return null;
    });
    if (!draft) return;
    if (draft.fields.length === 0) {
      setDraftNote("No fields could be inferred — enter at least one non-space run.");
    } else {
      setFields(draft.fields);
      setDraftNote("Draft only — review and edit field boundaries before saving.");
    }
  }

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    if (!name.trim()) {
      setError("schema name is required");
      return;
    }
    const cleanFields = fields
      .filter((f) => f.name.trim().length > 0)
      .map((f) => ({ ...f, name: f.name.trim() }));
    setSaving(true);
    try {
      const res = await api.saveSchema({ name, encoding, index_base: indexBase, fields: cleanFields });
      navigate(`/schemas/${encodeURIComponent(res.schema.name)}`, { state: { warnings: res.warnings } });
    } catch (err) {
      setError(err instanceof ApiRequestError ? err.message : "failed to save schema");
    } finally {
      setSaving(false);
    }
  }

  const isEdit = mode === "edit";

  return (
    <div>
      <header className="mb-5">
        <h1 className="font-display text-2xl font-semibold text-[var(--fg)]">
          {isEdit ? `Edit "${editName}"` : "Create schema"}
        </h1>
        <div className="text-sm text-[var(--muted)]">
          Ordered fixed-width fields · start + length, zero-indexed by default
        </div>
      </header>
      {error && <ErrorBanner message={error} />}

      {!isEdit && (
        <details className="mb-4 rounded-xl border border-[var(--border)] bg-[var(--surface)] p-4 shadow-[var(--shadow-sm)]">
          <summary className="cursor-pointer font-semibold">Infer a draft from a sample line</summary>
          <div className="mt-3">
            {draftNote && <WarnBanner message={draftNote} />}
            <Label htmlFor="sample">Paste one representative fixed-width line</Label>
            <Input
              id="sample"
              className="w-full"
              value={sample}
              onChange={(e) => setSample(e.target.value)}
              placeholder="0000000042Alice                         New York"
            />
            <p className="mt-2">
              <Button type="button" secondary onClick={inferDraft}>
                Infer draft
              </Button>
            </p>
            <p className="mt-1 text-xs text-[var(--muted)]">
              Draft only — review and edit field boundaries before saving.
            </p>
          </div>
        </details>
      )}

      <form onSubmit={onSubmit} className="max-w-3xl">
        <Label htmlFor="name">Schema name</Label>
        <Input
          id="name"
          required
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="customers_layout"
        />

        <Label htmlFor="encoding">Encoding</Label>
        <Input id="encoding" value={encoding} onChange={(e) => setEncoding(e.target.value)} />

        <Label htmlFor="index_base">Index base</Label>
        <Input
          id="index_base"
          type="number"
          min={0}
          value={indexBase}
          onChange={(e) => setIndexBase(Number(e.target.value))}
        />

        <h2 className="mt-6 mb-2 text-lg font-bold">Fields</h2>
        <div className="flex flex-col gap-2">
          {fields.map((f, i) => (
            <div key={i} className="flex items-end gap-2">
              <Input
                className="min-w-0 flex-1"
                placeholder="name"
                value={f.name}
                onChange={(e) => updateField(i, { name: e.target.value })}
              />
              <Input
                className="w-28 min-w-0"
                type="number"
                min={0}
                placeholder="start"
                value={f.start}
                onChange={(e) => updateField(i, { start: Number(e.target.value) })}
              />
              <Input
                className="w-28 min-w-0"
                type="number"
                min={1}
                placeholder="length"
                value={f.length}
                onChange={(e) => updateField(i, { length: Number(e.target.value) })}
              />
            </div>
          ))}
        </div>
        <p className="mt-2">
          <Button type="button" secondary onClick={addRow}>
            + Add field
          </Button>
        </p>
        <p className="mt-4">
          <Button type="submit" disabled={saving}>
            {isEdit ? "Update schema (new version)" : "Save schema"}
          </Button>
        </p>
      </form>
    </div>
  );
}
