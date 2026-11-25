# Step 16 — NDJSON / JSON Lines

**Phase:** v1 · **Depends on:** 04, 07, 09

## Goal
First-class support for newline-delimited JSON (`.ndjson`, `.jsonl`, log exports): each line is a record, files can be many GB, and one bad line must not stop the rest from working.

## Detection
- By extension `.ndjson`, `.jsonl`, `.ldjson` → NDJSON.
- By content for `.json` / unknown: if the first complete top-level value ends before EOF and is followed by a newline and another value start → NDJSON. Ask with a non-blocking banner "This looks like JSON Lines (one record per line). View as records? [Yes] [No, treat as JSON]" and remember the choice per file.
- A manual toggle in the View menu.

## Index (extension of step 04)
- The record table is `Vec<u64>` of line-start offsets for non-empty lines (built in parallel with `memchr` for `\n`; handle `\r\n`).
- Each record is indexed as an independent root: the node table stores records' containers with `parent = RECORD_ROOT(n)` and records carry their own start/end.
- **Per-line validation:** an invalid line is recorded in an `errors: Vec<(record_no, JsonError)>` list and indexing continues. The tree shows invalid records with a red ⚠ and the error text.
- Blank lines are skipped. A trailing newline at EOF is OK.
- The cache format (step 05) stores the record table and error list (bump `format_version`).

## UI
- The tree root is a virtual "Records (12,345,678)" node, grouped like big arrays: `[0 … 999]`. Labels show `#line` numbers.
- Raw view: Original mode by default (lines are natural); Pretty toggles per selected record.
- Search and JSONPath work across all records; JSONPath `$` refers to **each record** (so `$.level` returns the level from every record). Show this in the hint.
- Errors panel: "37 invalid lines" → list with line numbers and messages, click to jump.
- Paths shown as `#1204331 $.user.id` (record number + path inside the record).

## Acceptance criteria
- [ ] `logs_1g.ndjson` indexes within the same targets as JSON of the same size.
- [ ] A file with invalid lines sprinkled in: all valid records browsable, exact list of invalid lines with line/column.
- [ ] Search counts and JSONPath results equal a per-line serde oracle.
- [ ] `\r\n` files work. A JSON file with pretty multi-line objects is **not** misdetected as NDJSON.

## Tests
Detection unit tests (true and false positives), per-line oracle proptest, invalid-line fixtures, kittest for the records root and errors panel.
