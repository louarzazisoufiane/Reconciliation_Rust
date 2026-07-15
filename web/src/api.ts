import type {
  ManifestEntry,
  RunBuildRequest,
  RunCreateResponse,
  Schema,
  SchemaInfo,
  SchemaSaveResponse,
  ValidateRunRequest,
  ValidateRunResponse,
} from "./types";

export class ApiRequestError extends Error {}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api${path}`, {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: res.statusText }));
    throw new ApiRequestError(body.error ?? res.statusText);
  }
  return res.json() as Promise<T>;
}

export const api = {
  listSchemas: () => request<SchemaInfo[]>("/schemas"),

  getSchema: (name: string, version?: number) =>
    request<Schema>(`/schemas/${encodeURIComponent(name)}${version ? `?v=${version}` : ""}`),

  inferSchema: (sample: string) =>
    request<Schema>("/schemas/infer", {
      method: "POST",
      body: JSON.stringify({ sample }),
    }),

  saveSchema: (draft: Omit<Schema, "version"> & { version?: number }) =>
    request<SchemaSaveResponse>("/schemas", {
      method: "POST",
      body: JSON.stringify({ ...draft, version: draft.version ?? 0 }),
    }),

  validateRun: (req: ValidateRunRequest) =>
    request<ValidateRunResponse>("/runs/validate", {
      method: "POST",
      body: JSON.stringify(req),
    }),

  createRun: (req: RunBuildRequest) =>
    request<RunCreateResponse>("/runs", {
      method: "POST",
      body: JSON.stringify(req),
    }),

  listRuns: () => request<ManifestEntry[]>("/runs"),
};
