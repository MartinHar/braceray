//! Step 11: the path of every value (from `node_at_offset` + `path_of`, as the app gets
//! it) in each format resolves to that value with a reference implementation, and the
//! value's copy text (pretty / minified) parses to the same value.

mod common;

use std::collections::HashMap;

use braceray_core::copy::{CopyStyle, render};
use braceray_core::index::{IndexOptions, build_bytes};
use braceray_core::nav::{Navigator, Span};
use braceray_core::path::{PathFormat, decode_key, format_path};
use braceray_core::{CancelToken, Progress};
use common::{Seg, json_tree, layout, serde_value, serialize_with_values};
use proptest::prelude::*;
use serde_json::Value;

fn resolve(doc: &Value, path: &str, format: PathFormat) -> Option<Value> {
    match format {
        PathFormat::JsonPath => serde_json_path::JsonPath::parse(path)
            .unwrap_or_else(|e| panic!("{path:?} isn't valid RFC 9535 JSONPath: {e}"))
            .query(doc)
            .exactly_one()
            .ok()
            .cloned(),
        PathFormat::Pointer => doc.pointer(path).cloned(),
        _ => unreachable!(),
    }
}

/// The oracle path with keys decoded (encoded bodies → text).
fn decoded(path: &[Seg]) -> Vec<Result<String, u64>> {
    path.iter()
        .map(|s| match s {
            Seg::Key(k) => Ok(decode_key(format!("\"{k}\"").as_bytes())),
            Seg::Index(i) => Err(*i),
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn every_value_path_resolves_and_copies(tree in json_tree(), layout in layout()) {
        let (bytes, _, values) = serialize_with_values(&tree, &layout);
        // The generator writes numbers like `0.1e310` that serde can't hold.
        let parses = {
            use serde::Deserialize;
            let mut de = serde_json::Deserializer::from_slice(&bytes);
            de.disable_recursion_limit();
            Value::deserialize(&mut de).is_ok()
        };
        prop_assume!(parses);
        let doc = serde_value(&bytes);
        let index = build_bytes(&bytes, &IndexOptions::default(), &CancelToken::new(), &Progress::default()).unwrap();
        let nav = Navigator::new(&bytes, &index);
        // Duplicate keys resolve to the last member in serde: only check unique paths.
        let mut seen: HashMap<_, usize> = HashMap::new();
        for v in &values {
            *seen.entry(decoded(&v.path)).or_default() += 1;
        }
        let (cancel, progress) = (CancelToken::new(), Progress::default());
        for v in &values {
            let path = nav.path_of(&nav.node_at_offset(v.start as u64));
            let want = serde_value(&bytes[v.start..v.end]);
            let unique = seen[&decoded(&v.path)] == 1
                && v.path.iter().enumerate().all(|(i, _)| seen[&decoded(&v.path[..i])] == 1);
            if unique {
                for format in [PathFormat::JsonPath, PathFormat::Pointer] {
                    let p = format_path(&bytes, &path, format, "data");
                    let got = resolve(&doc, &p, format);
                    prop_assert_eq!(got.as_ref(), Some(&want), "{:?}", p);
                }
            }
            let span = Span { start: v.start as u64, end: v.end as u64 };
            for style in [CopyStyle::Pretty { indent: 2 }, CopyStyle::Minified, CopyStyle::Raw] {
                let text = render(&bytes, span, style, &cancel, &progress).unwrap();
                prop_assert_eq!(serde_value(text.as_bytes()), want.clone());
            }
            if let Value::String(s) = &want {
                let text = render(&bytes, span, CopyStyle::StringContents, &cancel, &progress).unwrap();
                prop_assert_eq!(&text, s);
            }
        }
    }
}
