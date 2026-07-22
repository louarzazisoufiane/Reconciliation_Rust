import type { ComparisonResponse, DeltaRow, Layout, NewLayout } from "./types";
export class ApiRequestError extends Error {}
async function request<T>(path: string, init?: RequestInit): Promise<T> { const res = await fetch(`/api${path}`, init); if (!res.ok) { const body = await res.json().catch(() => ({ error: res.statusText })); throw new ApiRequestError(body.error ?? res.statusText); } return res.json() as Promise<T>; }
export const api = {
  listLayouts: () => request<Layout[]>("/layouts"),
  createLayout: (layout: NewLayout) => request<Layout>("/layouts", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(layout) }),
  createComparison: (data: FormData) => request<ComparisonResponse>("/comparisons", { method: "POST", body: data }),
  getDelta: (id: string) => request<DeltaRow[]>(`/comparisons/${id}/delta`),
};
