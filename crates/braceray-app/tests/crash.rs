//! The panic hook of the real binary: `braceray --crash-test` panics at startup, before any
//! window opens; the hook must save a crash log in the settings folder.

use std::process::Command;

#[test]
fn panic_writes_a_crash_log_to_the_config_dir() {
    let home = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_braceray"))
        .arg(braceray_app::crash::CRASH_TEST_FLAG)
        .env("HOME", home.path())
        .env("APPDATA", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env(braceray_app::crash::NO_DIALOG_ENV, "1")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Braceray hit a bug. Log saved to"),
        "{stderr}"
    );

    let dir = braceray_app::settings::default_path_in(home.path())
        .parent()
        .unwrap()
        .to_owned();
    let logs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("crash-"))
        .collect();
    assert_eq!(logs.len(), 1, "{dir:?}");
    // Windows writes the panic location with backslashes.
    let text = std::fs::read_to_string(logs[0].path())
        .unwrap()
        .replace('\\', "/");
    assert!(
        text.contains("message: crash test requested with --crash-test"),
        "{text}"
    );
    assert!(
        text.contains("location: crates/braceray-app/src/main.rs"),
        "{text}"
    );
    assert!(text.contains("backtrace:"));
}
