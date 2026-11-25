# Step 12 — MVP hardening, benchmarks & release v0.1.0

**Phase:** MVP · **Depends on:** 01–11

## Goal
Prove the MVP meets every performance target, close robustness gaps, and ship free downloadable builds for Windows and macOS from GitHub Releases.

## 12a — Performance verification
1. `tools/fixtures` gets a `bench-big` command: generates (if missing) the 2 GB pretty + minified fixtures, then measures with warm and (where possible) cold cache:
   open → first content, full index, re-open from cache, search cs / ci (common + rare needle), expand root, peak RSS.
   Output a Markdown table to append to `docs/PERF.md` (machine info: CPU, cores, RAM, OS, disk).
2. Compare against `overview.md` targets. For each miss: profile (`cargo flamegraph` / `samply`, both free), fix, and re-measure. Record what was changed in `DECISIONS.md`.
3. Frame-time check: a debug overlay (`Ctrl+Shift+D`) showing frame ms (p50 / p99 over the last 300 frames). Verify < 16 ms p99 while scrolling each view on the 2 GB file.

## 12b — Robustness pass
- Run all fuzz targets for ≥ 10 minutes each; fix every crash and add its input to the regression corpus.
- Open every fixture in the catalogue (including encoding edge cases, `invalid_*`, 5 GB sparse) through the UI via egui_mcp; no panics, clear messages.
- Close a tab during indexing, during search, during copy: no leaks (memory returns to baseline), no panics.
- Modify the file during indexing → reload banner, no crash.
- Set `RUST_BACKTRACE=1` panic hook: on panic, write a crash log to the config dir and show a dialog "Braceray hit a bug. Log saved to … [Open issue on GitHub]" (opens a prefilled issue URL in the browser; nothing is sent automatically).

## 12c — Branding
- Final app icon: `{ }` braces with a horizontal cyan scan beam on a dark rounded square. Generate the SVG, export PNG 1024 / 512 / 256 / 128 / 64 / 32 / 16, `.icns` (macOS, built with `iconutil` on the macOS runner or the `icns` crate) and `.ico` (Windows, `ico` crate). Set the window icon via eframe's `ViewportBuilder::with_icon`.
- About dialog: name, version, tagline, MIT license, GitHub link, third-party licenses viewer.

## 12d — Packaging & release workflow (`.github/workflows/release.yml`)
Trigger: push of tag `v*`. Jobs:
1. **Windows x64** (`windows-latest`): `cargo build --release --target x86_64-pc-windows-msvc`. Embed the icon and version info (`winresource` crate in `build.rs`). Zip `Braceray.exe` + `LICENSE` + `THIRD_PARTY_LICENSES.md` + `README.txt` → `Braceray-<ver>-windows-x64.zip`. (Optional: `aarch64-pc-windows-msvc` zip.)
2. **macOS universal** (`macos-latest`): build `aarch64-apple-darwin` and `x86_64-apple-darwin`, then `lipo -create` into one binary. Assemble `Braceray.app` (`Contents/MacOS/braceray`, `Contents/Info.plist` from `packaging/macos/Info.plist` with version filled in, `Contents/Resources/Braceray.icns`). Info.plist must declare `CFBundleDocumentTypes` for `public.json` (viewer role) and `LSMinimumSystemVersion` 11.0.
   **Ad-hoc sign** (free, no Apple account): `codesign --force --deep --sign - Braceray.app`. Create the DMG with `hdiutil create -volname Braceray -srcfolder <dir with app + Applications symlink> -ov -format UDZO Braceray-<ver>-macos-universal.dmg`.
3. **Linux x64** (bonus): tar.gz of the binary.
4. **Publish:** generate `SHA256SUMS.txt`, create the GitHub Release with `gh release create` (uses the built-in `GITHUB_TOKEN`), and attach all artifacts. The body is the changelog section for the tag.
- No paid services, no secrets other than `GITHUB_TOKEN`.

## 12e — Docs
`README.md`: screenshot/GIF (record with any free tool), features, **Install** section:
- **Windows:** download the zip → extract → run `Braceray.exe`. If SmartScreen appears: *More info → Run anyway* (the app is not code-signed because it's a free project).
- **macOS:** open the dmg → drag to Applications → first launch: if macOS says it can't verify the developer, open *System Settings → Privacy & Security* → scroll down → *Open Anyway*. Or in Terminal: `xattr -dr com.apple.quarantine /Applications/Braceray.app`.
- Build from source, keyboard shortcuts, performance table, privacy statement ("Braceray never sends your data anywhere"), license.

## Acceptance criteria
- [ ] Every MVP target in `overview.md` is met on at least one reference machine, and the numbers are in `docs/PERF.md`.
- [ ] Fuzzing finds no crashes in 10 min per target.
- [ ] `git tag v0.1.0 && git push --tags` produces a GitHub Release with Windows zip, macOS dmg, Linux tar.gz and checksums.
- [ ] Downloaded builds launch on a clean Windows machine and a Mac (Apple Silicon) following the README steps.
- [ ] The release checklist in `testing-strategy.md` is completed and pasted into the release PR or issue.
- [ ] `CHANGELOG.md` has a `[0.1.0]` section.
