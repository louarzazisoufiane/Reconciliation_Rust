export interface LayoutField { name: string; start: number; end: number; is_primary_key: boolean; }
export interface Layout { id: string; name: string; fields: LayoutField[]; }
export interface NewLayout { name: string; fields: LayoutField[]; }
export interface ComparisonResponse { id: string; old_rows: number; new_rows: number; added: number; removed: number; modified: number; }
export interface ComparisonRun {
  id: string;
  run_index: number;
  run_name: string;
  created_at: string;
  processing_duration_ms: number | null;
  processing_started_at: string | null;
  processing_completed_at: string | null;
  old_layout_name: string;
  new_layout_name: string;
  old_date_of_download: string | null;
  new_date_of_download: string | null;
  old_origin_file_name: string | null;
  new_origin_file_name: string | null;
}
export interface DeltaRow { composite_primary_key: string; change_type: "modified" | "added" | "removed"; old_data: Record<string, string> | null; new_data: Record<string, string> | null; changed_fields: Record<string, { old: string | null; new: string | null }>; }
