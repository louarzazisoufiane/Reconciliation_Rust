export interface LayoutField { name: string; start: number; end: number; is_primary_key: boolean; }
export interface Layout { id: string; name: string; fields: LayoutField[]; }
export interface NewLayout { name: string; fields: LayoutField[]; }
export interface ComparisonResponse { id: string; old_rows: number; new_rows: number; added: number; removed: number; modified: number; }
export interface DeltaRow { composite_primary_key: string; change_type: "modified" | "added" | "removed"; old_data: Record<string, string> | null; new_data: Record<string, string> | null; changed_fields: Record<string, { old: string | null; new: string | null }>; }
