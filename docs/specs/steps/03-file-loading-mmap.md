# Step 03 — File loading (memory-mapped)

**Phase:** MVP · **Depends on:** 01, 02

## Goal
`braceray_core::Document`: opens any file in O(1) time via memory mapping, detects encoding problems and exposes the bytes safely. This is the foundation that everything else reads from.

## API
```rust
pub struct Document { /* path, mmap, identity, content_start */ }

impl Document {
    pub fn open(path: &Path) -> Result<Arc<Document>, Error>;
    pub fn bytes(&self) -> &[u8];               // JSON content (after a UTF-8 BOM, if any)
    pub fn len(&self) -> u64;
    pub fn path(&self) -> &Path;
    pub fn identity(&self) -> &FileIdentity;     // canonical path, size, mtime, xxh3(head 1 MiB + tail 1 MiB)
    pub fn has_changed_on_disk(&self) -> bool;   // cheap: metadata only (size + mtime)
    pub fn sniff(&self) -> Sniff;                // Json | NdJson | Unknown (refined in step 16)
}
```

## Implementation notes
- Use `memmap2::Mmap` (read-only). `// SAFETY:` comment explaining the file-changed risk and how we mitigate it (architecture.md §9).
- Call `mmap.advise(Advice::Sequential)` before the index build, then `Advice::Random` afterwards (where supported; no-op elsewhere).
- Empty file: return `Error::EmptyFile` with the message "This file is empty."
- Encoding sniff on the first 4 bytes and a 4 KiB sample:
  - UTF-8 BOM `EF BB BF` → skip it (`content_start = 3`).
  - UTF-16 LE/BE BOM, or many `00` bytes in the sample → `Error::Encoding(Utf16Le|Utf16Be)`: "This file is UTF-16. Braceray supports UTF-8 JSON. Convert it with: …" (give a short command for macOS and Windows).
  - Binary detection: > 1 % NUL or control bytes in the sample (outside whitespace) → `Error::Encoding(Binary)`.
- First non-whitespace byte must be `{`, `[`, `"`, a digit, `-`, `t`, `f` or `n`. Otherwise give an early helpful error (a full validation follows in step 04).
- Files > 4 GB must work. Use `u64` offsets everywhere and test with a 5 GB sparse file (created with `set_len`, all spaces + `[]`; generation is instant).
- Files on network drives: mmap may fail. Fall back to an error message "Can't map this file (network drive?). Copy it locally." Don't silently read it into RAM.
- `FileIdentity` hashing uses xxh3 on head + tail only (constant time).

## Acceptance criteria
- [ ] Opening the 2 GB fixture takes < 50 ms (measure in a test with `Instant`, `#[ignore]`d unless fixtures exist).
- [ ] Process memory after open increases by < 5 MB (mapped pages not counted until touched).
- [ ] UTF-8 BOM is skipped; UTF-16 and binary files return the right error variant and message.
- [ ] Empty and whitespace-only files give clear errors.
- [ ] A 5 GB sparse file opens and `len()` is correct.
- [ ] `has_changed_on_disk()` turns true after appending a byte or touching mtime.

## Tests
Unit tests for every bullet above using `tempfile`. A doc-test example on `Document::open`.
