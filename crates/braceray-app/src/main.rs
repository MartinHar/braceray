// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use braceray_app::{AppConfig, BracerayApp};
use eframe::egui;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Default address of the debug inspection server (egui_mcp connects here).
#[cfg(debug_assertions)]
const INSPECTION_ADDR: &str = "127.0.0.1:5719";

fn main() -> eframe::Result {
    // `braceray [file]`: the first argument that isn't a flag is opened.
    let file: Option<PathBuf> = std::env::args_os()
        .skip(1)
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
