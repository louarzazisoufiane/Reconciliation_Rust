// Mirrors the JSON shapes emitted by `recon-web`'s handlers (see
// crates/recon-web/src/main.rs), which in turn mirror the on-disk YAML
// schema/run-config shapes documented in CLAUDE.md — snake_case throughout so
// the wire format matches the config a human could hand-write.

export interface Field {
  name: string;
  start: number;
  length: number;
}

export interface Schema {
  name: string;
  version: number;
  encoding: string;
  index_base: number;
  fields: Field[];
}

export interface SchemaInfo {
  name: string;
  latest_version: number;
  field_count: number;
  created_at: string;
}

export interface SchemaRef {
  name: string;
  version: number;
}

export interface Normalization {
  trim?: boolean;
  strip_leading_zeros?: boolean;
  unify_null?: boolean;
  case_fold?: boolean;
}

export interface SchemaSaveResponse {
  schema: Schema;
  warnings: string[];
}

export interface ValidateRunRequest {
  schema_a: SchemaRef;
  schema_b: SchemaRef;
}

export interface ValidateRunResponse {
  common_columns: string[];
}

export interface RunBuildRequest {
  run_name: string;
  path_a: string;
  path_b: string;
  schema_a: SchemaRef;
  schema_b: SchemaRef;
  key: string;
  compare_columns: string[];
  normalization: Record<string, Normalization>;
}

export interface Summary {
  run_id: string;
  run_name: string;
  timestamp: string;
  key: string;
  compare_columns: string[];
  source_a_path: string;
  source_b_path: string;
  schema_a_ref: string;
  schema_b_ref: string;
  rows_a: number;
  rows_b: number;
  only_in_a: number;
  only_in_b: number;
  changed: number;
  matched: number;
  dup_keys_a: number;
  dup_keys_b: number;
  match_rate: number;
  pass: boolean;
}

export interface RunCreateResponse {
  run_id: string;
  report_url: string;
  summary: Summary;
}

export interface ManifestEntry {
  run_id: string;
  run_name: string;
  timestamp: string;
  report_html: string;
  sidecar: string;
  key: string;
  rows_a: number;
  rows_b: number;
  only_in_a: number;
  only_in_b: number;
  changed: number;
  matched: number;
  dup_keys_a: number;
  dup_keys_b: number;
  match_rate: number;
  pass: boolean;
}

export interface ApiError {
  error: string;
}
