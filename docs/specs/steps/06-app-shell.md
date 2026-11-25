# Step 06 — App shell (window, open, theme, settings, jobs)

**Phase:** MVP · **Depends on:** 03, 04, 05

## Goal
The real application frame: open files every possible way, show indexing progress, theme, settings, recent files, tabs and a status bar. It's wired to core jobs without ever blocking the UI. Also: egui_mcp inspection for Claude-driven UI testing.

## Tasks
1. **Job plumbing** (`braceray-core::job`): `CancelToken`, `Progress`, `JobHandle<T>`, `JobEvent<T>` per architecture.md §5, plus a repaint callback. In the app, `OpenDocument` owns the index job and polls it each frame.
2. **Opening files**
   - Toolbar button and `Ctrl/Cmd+O` → `rfd::FileDialog` (filters: JSON `*.json`, NDJSON `*.ndjson *.jsonl`, All files). Run the dialog so it doesn't freeze rendering (rfd async API or a thread).
   - Drag & drop onto the window (`ctx.input(|i| i.raw.dropped_files)`), with a drop-zone highlight while hovering.
   - Command line: `braceray path/to/file.json` (first arg).
   - macOS "Open With" / double-click: investigate whether the current eframe/winit version delivers open-file events. If yes, implement; if not, record it in `docs/DECISIONS.md` as a follow-up. Don't block the step on it.
   - Multiple files → tabs (closable, `Ctrl/Cmd+W`, middle-click). Re-opening an already-open file focuses its tab.
3. **Empty state**: large drop zone, "Open file…" button, recent files list (click to open; missing files shown greyed with a "Remove" action).
4. **Indexing UX**: as soon as the file is mapped, show file name, size and a progress bar with percentage, speed (GB/s) and ETA, plus a Cancel button. The tree's top level appears as soon as the root's first children are known (step 07 hooks in here). On completion: "Indexed 2.00 GB in 3.8 s · 12,345,678 nodes".
5. **Errors**: banner component with message, "Details" (copyable) and actions (e.g. "Open anyway" shows the valid prefix for JSON errors).
6. **Status bar**: file size · index state (from cache / built + time) · selected line:col · match count (later) · process memory (via `sysinfo` or a platform call, refreshed every 2 s).
7. **Theme**: Dark / Light / System (follow the OS; `ctx.options` / `ThemePreference`). A clean, modern custom `Visuals`: one accent colour (cyan "ray" `#22D3EE` on dark, `#0891B2` on light), 6 px rounding, comfortable spacing. Monospace font for data (the bundled egui monospace is fine).
8. **Settings window** (gear icon): theme; index cache on/off (default **on**), max cache size, min file size, "Clear cache (X GB)"; search defaults (filled in steps 09–13); max stored search results; tree group size (default 1000). Persist with eframe storage (`persistence` feature) or a JSON file in the config dir. Validate on load and reset bad values to defaults.
9. **Recent files**: last 15, persisted, with "Clear recent".
10. **File-change watcher**: check `has_changed_on_disk()` on focus and every 5 s → banner "File changed on disk. Reload?".
11. **Inspection for Claude Code (debug builds only)**: enable egui's inspection plugin (`egui_inspection` crate, `InspectionPlugin`) behind `#[cfg(debug_assertions)]` or a `--inspect` flag. Document in `README.md` → *Development* how to connect: install the `egui_mcp` server per the current egui release notes and register it with `claude mcp add egui egui-mcp`. Make sure every interactive widget has a clear accessible label (also needed for kittest).
12. **Keyboard map** from architecture.md §7, shown in a "Keyboard shortcuts" dialog (`?` or Help menu).

## Acceptance criteria
- [ ] Opening the 2 GB fixture: the window stays responsive the whole time (drag the window and resize during indexing, no stutter); progress updates smoothly; Cancel works.
- [ ] Drag & drop, dialog, CLI arg and recent files all open files.
- [ ] Opening a second file opens a tab; switching tabs is instant.
- [ ] Invalid JSON shows the banner with line/column; UTF-16 and empty files show their friendly messages.
- [ ] Theme switch is instant and persisted; "System" follows the OS.
- [ ] Settings persist across restarts; the cache toggle really disables cache writes.
- [ ] Reopening a cached file shows "from cache" in the status bar.
- [ ] Debug build can be driven via egui_mcp (Claude Code: open a fixture through the UI and read the status bar).

## Tests (egui_kittest)
Open via a test hook → progress then done state; error banner for an invalid fixture; theme toggle; settings round-trip; tabs open/close.
