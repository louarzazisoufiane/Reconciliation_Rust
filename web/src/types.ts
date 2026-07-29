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
  old_layout_name: string;
  new_layout_name: string;
  old_date_of_download: string | null;
  new_date_of_download: string | null;
  old_origin_file_name: string | null;
  new_origin_file_name: string | null;
  old_row_count: number | null;
  new_row_count: number | null;
  added: number;
  removed: number;
  modified: number;
}
export interface DeltaRow { composite_primary_key: string; change_type: "modified" | "added" | "removed"; old_data: Record<string, string> | null; new_data: Record<string, string> | null; changed_fields: Record<string, { old: string | null; new: string | null }>; }
export type ScheduleFrequency = "one_time" | "daily" | "weekly" | "monthly";
export type ScheduleStatus = "pending" | "running" | "completed" | "failed";
export interface ScheduledTask { id: number; name: string; frequency: ScheduleFrequency; run_at: string; old_path: string; new_path: string; old_layout_id: string; new_layout_id: string; archive_path: string; status: ScheduleStatus; created_at: string; last_run_at: string | null; error_message: string | null; }
export interface NewScheduledTask { name: string; frequency: ScheduleFrequency; run_at: string; old_path: string; new_path: string; old_layout_id: string; new_layout_id: string; archive_path: string; }
