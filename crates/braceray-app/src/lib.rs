//! Braceray desktop app (egui). The binary in `main.rs` is a thin launcher around [`BracerayApp`].

mod app;
pub mod format;
pub mod settings;
pub mod tab;
pub mod theme;
pub mod tree_view;
pub mod ui;

pub use app::{AppConfig, BracerayApp, DROP_HINT};
