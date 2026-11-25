# Step 11 — Breadcrumbs, copy path & copy value

**Phase:** MVP · **Depends on:** 07, 08

## Goal
Always show where you are, and let users copy the path or value of anything in the formats they need.

## Breadcrumb bar
- Under the toolbar: `$ › orders › [48213] › customer › email`
- Each segment is clickable (selects that ancestor). Hovering a segment shows a dropdown of its siblings (first 200 + "…") for quick lateral navigation.
- Long paths collapse in the middle (`$ › orders › … › customer › email`); click `…` to expand.
- A copy button at the right end with a format dropdown (remembers the last choice).

## Path formats (`braceray-core::path`)
| Format | Example |
|---|---|
| JSONPath (RFC 9535) | `$.orders[48213].customer.email` · `$['weird key'][0]` |
| jq | `.orders[48213].customer.email` · `.["weird key"][0]` |
| JavaScript | `data.orders[48213].customer.email` · `data["weird key"][0]` (root name configurable, default `data`) |
| JSON Pointer (bonus) | `/orders/48213/customer/email` (with `~0` / `~1` escaping) |

Quoting rules: use dot notation only for keys matching `^[A-Za-z_$][A-Za-z0-9_$]*$` (and for JS, not a reserved word). Otherwise bracket notation with proper escaping of quotes, backslashes and control chars. Keys are decoded from JSON escapes before formatting.

## Copy value / subtree
- Actions: **Copy value** (as in the file), **Copy formatted** (pretty), **Copy minified**, **Copy key**.
- For a string, also **Copy string contents** (decoded, without quotes).
- Size guard: if the value is > 50 MB (setting), ask: "This value is 1.2 GB. Copying it may use a lot of memory. Export to file instead?" → [Export…] [Copy anyway] [Cancel]. (Export to file: a simple save dialog writing the raw byte range. The full export feature comes in step 17.)
- Pretty/minified formatting of large values runs as a job with progress (streaming formatter in `pretty.rs`, no DOM).
- Clipboard via `arboard`. Toast: "Copied JSONPath" / "Copied 12.4 MB".
- Shortcuts: `Ctrl/Cmd+Shift+C` copy path (last format), `Ctrl/Cmd+C` copy value when the tree has focus.

## Acceptance criteria
- [ ] Paths for keys with spaces, quotes, dots, unicode, empty string, numeric-looking keys and reserved words are correct in every format (table-driven tests).
- [ ] Each copied path, evaluated by a reference implementation over `serde_json::Value` (JSONPath / Pointer in tests; jq if available in CI, optional), returns the selected value.
- [ ] Copying a 10 MB object takes < 300 ms; copy formatted equals the serde pretty output (whitespace style aside: compare parsed values).
- [ ] The size guard triggers above the limit.
- [ ] Breadcrumb navigation and sibling dropdown work on a 1M-child array.

## Tests
Core: table tests for path formatting and escaping, proptest (random keys → JSONPath → resolve → same value). kittest: breadcrumb click selects the ancestor; copy action calls a mock clipboard with the expected string.
