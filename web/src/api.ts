import type { ComparisonResponse, ComparisonRun, DeltaRow, Layout, NewLayout, NewScheduledTask, ScheduledTask } from "./types";
export class ApiRequestError extends Error {}
async function request<T>(path: string, init?: RequestInit): Promise<T> { const res = await fetch(`/api${path}`, init); if (!res.ok) { const body = await res.json().catch(() => ({ error: res.statusText })); throw new ApiRequestError(body.error ?? res.statusText); } if (res.status === 204) return undefined as T; return res.json() as Promise<T>; }
export const api = {
  listLayouts: () => request<Layout[]>("/layouts"),
  createLayout: (layout: NewLayout) => request<Layout>("/layouts", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(layout) }),
  listComparisons: () => request<ComparisonRun[]>("/comparisons"),
  createComparison: (data: FormData) => request<ComparisonResponse>("/comparisons", { method: "POST", body: data }),
  getDelta: (id: string) => request<DeltaRow[]>(`/comparisons/${id}/delta`),
  listScheduled: (status = "pending") => request<ScheduledTask[]>(`/scheduled?status=${encodeURIComponent(status)}`),
  createScheduled: (task: NewScheduledTask) => request<ScheduledTask>("/scheduled", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(task) }),
  updateScheduled: (id: string, task: NewScheduledTask) => request<ScheduledTask>(`/scheduled/${id}`, { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify(task) }),
  deleteScheduled: (id: string) => request<void>(`/scheduled/${id}`, { method: "DELETE" }),
  runScheduledNow: (id: string) => request<void>(`/scheduled/${id}/run-now`, { method: "POST" }),
};
