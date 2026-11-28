//! Settings parsing, validation and persistence.

use super::*;

#[test]
fn round_trip() {
    let mut s = Settings {
        theme: ThemeChoice::Light,
        index_cache_enabled: false,
        tree_group_size: 500,
        path_format: PathFormat::Jq,
        js_root: "window.doc".to_owned(),
        copy_limit_bytes: 8 << 20,
        show_raw: false,
        ..Settings::default()
    };
    s.push_recent(Path::new("/a.json"));
    s.push_recent(Path::new("/b.json"));
    assert_eq!(Settings::from_json(&s.to_json()), s);
}

#[test]
fn panes_are_never_both_hidden() {
    let s = Settings::from_json(r#"{"show_tree": false, "show_raw": false}"#);
    assert!(s.show_tree && s.show_raw);
    let s = Settings::from_json(r#"{"show_tree": false}"#);
    assert!(!s.show_tree && s.show_raw);
}

#[test]
fn bad_values_reset_one_by_one() {
    let s = Settings::from_json(
        r#"{"theme": "purple", "index_cache_enabled": false, "index_cache_max_bytes": 5,
            "tree_group_size": "big", "max_stored_results": 2000, "extra": 1}"#,
    );
    let d = Settings::default();
    assert_eq!(s.theme, d.theme);
    assert!(!s.index_cache_enabled);
    assert_eq!(s.index_cache_max_bytes, d.index_cache_max_bytes);
    assert_eq!(s.tree_group_size, d.tree_group_size);
    assert_eq!(s.max_stored_results, 2000);
    assert_eq!(Settings::from_json("not json"), d);
    assert_eq!(Settings::from_json("[1, 2]"), d);
    assert_eq!(Settings::from_json(""), d);
}

#[test]
fn copy_settings_are_validated() {
    let s = Settings::from_json(
        r#"{"path_format": "javascript", "js_root": "1bad", "copy_limit_bytes": 12}"#,
    );
    let d = Settings::default();
    assert_eq!(s.path_format, PathFormat::JavaScript);
    assert_eq!(s.js_root, d.js_root);
    assert_eq!(s.copy_limit_bytes, d.copy_limit_bytes);
    let s = Settings::from_json(r#"{"path_format": "xpath"}"#);
    assert_eq!(s.path_format, PathFormat::JsonPath);
    for ok in ["data", "$json", "window.data", "_", "a1"] {
        assert!(valid_js_root(ok), "{ok}");
    }
    for bad in ["", "1a", ".a", "a.", "a..b", "a b", "a[0]", "a;alert(1)"] {
        assert!(!valid_js_root(bad), "{bad}");
    }
    assert!(!valid_js_root(&"a".repeat(MAX_JS_ROOT + 1)));
}

#[test]
fn search_history_is_capped_deduplicated_and_persisted() {
    let mut s = Settings::default();
    for i in 0..30 {
        s.push_history(&format!("q{i}"));
    }
    assert!(s.push_history("q5"));
    assert!(!s.push_history("q5"));
    assert!(!s.push_history(""));
    assert_eq!(s.search_history.len(), MAX_HISTORY);
    assert_eq!(s.search_history[0], "q5");
    assert_eq!(s.search_history[1], "q29");
    let back = Settings::from_json(&s.to_json());
    assert_eq!(back.search_history, s.search_history);
    let bad = Settings::from_json(r#"{"search_history": ["a", "", "a", "b"]}"#);
    assert_eq!(bad.search_history, ["a", "b"]);
}

#[test]
fn recent_list_is_capped_and_deduplicated() {
    let mut s = Settings::default();
    for i in 0..20 {
        s.push_recent(Path::new(&format!("/f{i}.json")));
    }
    s.push_recent(Path::new("/f10.json"));
    assert_eq!(s.recent_files.len(), MAX_RECENT);
    assert_eq!(s.recent_files[0], Path::new("/f10.json"));
    assert_eq!(
        s.recent_files
            .iter()
            .filter(|p| p.ends_with("f10.json"))
            .count(),
        1
    );

    let json = r#"{"recent_files": ["/a", "/a", "", "/b"]}"#;
    assert_eq!(
        Settings::from_json(json).recent_files,
        vec![PathBuf::from("/a"), PathBuf::from("/b")]
    );
}

#[test]
fn store_saves_in_background_and_loads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("settings.json");
    let store = SettingsStore::new(Some(path.clone()));
    assert_eq!(store.load(), Settings::default());
    let s = Settings {
        theme: ThemeChoice::Dark,
        ..Settings::default()
    };
    store.save(&Settings::default());
    store.save(&s);
    store.flush();
    assert_eq!(SettingsStore::new(Some(path)).load(), s);
    let leftovers = std::fs::read_dir(dir.path().join("sub")).unwrap().count();
    assert_eq!(leftovers, 1, "no temp files left");
}

#[test]
fn default_path_is_absolute() {
    if let Some(p) = default_path() {
        assert!(p.is_absolute());
        assert!(p.ends_with("settings.json"));
    }
}
