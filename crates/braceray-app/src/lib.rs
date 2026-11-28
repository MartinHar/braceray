//! Braceray desktop app (egui). The binary in `main.rs` is a thin launcher around [`BracerayApp`].

pub mod alloc_stats;
mod app;
pub mod copy_job;
pub mod crash;
pub mod format;
pub mod panes;
pub mod raw_view;
pub mod search_view;
pub mod settings;
pub mod tab;
mod tab_search;
pub mod theme;
pub mod tree_view;
pub mod ui;

pub use app::{AppConfig, BracerayApp, DROP_HINT};
