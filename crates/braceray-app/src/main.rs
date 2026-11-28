// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use braceray_app::{AppConfig, BracerayApp, crash};
use eframe::egui;

#[cfg(not(feature = "alloc-stats"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Same allocator plus heap counters (measurement builds only, ADR 027).
#[cfg(feature = "alloc-stats")]
#[global_allocator]
static GLOBAL: braceray_app::alloc_stats::Counting = braceray_app::alloc_stats::Counting;

/// Default address of the debug inspection server (egui_mcp connects here).
#[cfg(debug_assertions)]
const INSPECTION_ADDR: &str = "127.0.0.1:5719";

fn main() -> eframe::Result {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // `braceray --crash-report <log>`: the dialog process started by the panic hook.
    if let Some(i) = args.iter().position(|a| a == crash::CRASH_REPORT_FLAG) {
        if let Some(log) = args.get(i + 1) {
            crash::run_dialog(std::path::Path::new(log));
        }
        return Ok(());
    }
    crash::install(braceray_app::settings::default_path().and_then(|p| p.parent().map(Into::into)));
    if args.iter().any(|a| a == crash::CRASH_TEST_FLAG) {
        panic!("crash test requested with {}", crash::CRASH_TEST_FLAG);
    }

    #[cfg(feature = "alloc-stats")]
    std::thread::spawn(|| {
        // Measurement builds (ADR 027) log the heap every 2 s for scripted runs.
        loop {
            if let Some((live, peak)) = braceray_app::alloc_stats::heap() {
                eprintln!("alloc-stats: heap {live} B, peak {peak} B");
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });

    // `braceray [file]`: the first argument that isn't a flag is opened.
    let file: Option<PathBuf> = args
        .into_iter()
        .find(|a| !a.to_string_lossy().starts_with("--"))
        .map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Braceray")
            .with_app_id("braceray")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([900.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Braceray",
        options,
        Box::new(move |cc| {
            #[cfg(debug_assertions)]
            attach_inspection(&cc.egui_ctx);
            #[cfg(not(debug_assertions))]
            let _ = cc;
            let mut app = BracerayApp::new(AppConfig::from_env());
            if let Some(file) = file {
                app.open_path(file);
            }
            Ok(Box::new(app))
        }),
    )
}

/// Debug builds serve the egui inspection protocol for Claude Code's `egui_mcp` server on
/// `127.0.0.1:5719`. `EGUI_INSPECTION=0` turns it off, `EGUI_INSPECTION=<addr>` moves it.
#[cfg(debug_assertions)]
fn attach_inspection(ctx: &egui::Context) {
    let addr = match std::env::var("EGUI_INSPECTION") {
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => return,
        Ok(v) if v.contains(':') => v,
        _ => INSPECTION_ADDR.to_owned(),
    };
    ctx.add_plugin(egui_inspection::InspectionPlugin::new(Some(
        "Braceray".to_owned(),
    )));
    match egui_inspection::serve(ctx, &addr) {
        Ok(()) => eprintln!("egui inspection listening on {addr}"),
        Err(e) => eprintln!("egui inspection: can't listen on {addr}: {e}"),
    }
}
