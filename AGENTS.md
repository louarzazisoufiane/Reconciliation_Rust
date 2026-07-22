You are a senior Rust backend engineer with deep experience in PostgreSQL, file-processing pipelines, and designing schemas for data that has to be defined dynamically by end users rather than fixed at compile time.

<context>
 I want to add a new feature to The project : a fixed-width file comparison system built around three tables — old, new, and delta. only use the web UI that is in the old project and discard everything else.
</context>

<feature_requirements>
1. **Layout definition (per load, not fixed in code).** Before loading a file, the user defines its layout through a form in the UI. Each row in the form specifies: a field name, a start position, an end position, and a checkbox marking whether that field is part of the primary key. A file's primary key can be composite — built by concatenating the substrings of every field flagged as "part of primary key," in the order they appear in the layout. The layout is not hardcoded anywhere but the user can choose either one of the already made layouts or the new layout which will be saved; the user can define a completely different layout for every load.

2. **Two separate files, two separate tables.** The user uploads two distinct fixed-width files: an "old" file and a "new" file, each potentially with its own layout and each with its own meta_data Date_of_Download , origin_file_name , an ID and The primary key. Parse each file according to the layout supplied for it, and load the result into two separate Postgres tables (old and new). Since the layout can differ between loads, design the schema strategy so it doesn't break when the field set changes from one load to the next — think through whether that means generating table structure dynamically per load, or storing parsed rows in a more flexible format (e.g., a fixed set of key columns plus a flexible column for the rest), and explicitly state which approach you're choosing and why before you implement it.

3. **Delta table.** Once both old and new are loaded, compute a delta by matching rows on the composite primary key:
   - Rows whose key exists in both tables but whose field values differ: record them in this table as modified, showing the old value and new value side by side for every field that changed.
   - Rows whose key exists only in the old table: record them as removed.
   - Rows whose key exists only in the new table: record them as added.
   Include a status/change-type column so each delta row is clearly marked as modified, added, or removed.

4. **Full feature, not just the database layer.** Build the whole thing end to end: the UI for defining the layout (rows of field name / start / end / is-primary-key), the file upload flow for the old and new files, the Rust backend logic that parses the fixed-width files against the submitted layout, the Postgres schema and migrations, and the comparison logic that produces the delta table.

5. **Performance.** Fixed-width files can be large. Favor efficient loading (batch inserts or COPY-style bulk loading over Postgres) rather than row-by-row inserts, and parse files in a streaming fashion rather than loading the whole file into memory if practical.

6. Add Docker support for PostgreSQL to this existing Rust project. Add a docker-compose.yml service running PostgreSQL, using environment variables for the database name, user, and password (with local-dev defaults in a .env file) instead of hardcoded values, and persist data with a named volume. Make sure the target database is created automatically if it doesn't already exist when the app starts, using whichever approach fits the project's existing database connection code cleanly. Check how the project currently connects to Postgres before adding anything, and match that existing pattern rather than introducing a separate one.

Think before answering (maximum reasoning)
</feature_requirements>

<before_you_start>
Inspect the existing project first — its web framework (e.g. actix-web, axum, rocket), its database access layer (e.g. sqlx, diesel, tokio-postgres), its existing migration setup, and its frontend approach. Match your implementation to those existing conventions rather than introducing a new pattern. If anything about the current codebase's structure or conventions is ambiguous or you can't determine it from the code, ask me before writing the implementation rather than guessing.
</before_you_start>

<deliverables>
- Postgres migration(s) for whatever schema strategy you settle on for old/new/delta.
- Rust backend code: the endpoint(s)/handler(s) for submitting a layout, uploading a file, parsing it, loading it, and triggering delta computation.
- The UI for the layout-definition form and file upload, in whatever frontend approach the existing project already uses.
- A short explanation of the schema strategy you chose for handling variable layouts, and why.
</deliverables>

Before you finish, re-read your implementation against every requirement above and confirm the composite key logic, the modified/added/removed delta logic, and the dynamic-layout handling all actually hold up — not just for the happy path.

Think before answering 