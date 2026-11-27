//! The search bar: `[▾][search text…][Aa][ab|][.*][All] count ‹ › ✕ [Results]`.
//! The text and toggles belong to the app (one bar for every tab); each tab runs the
//! settled query on its own file.

use std::time::{Duration, Instant};

use braceray_core::search::SearchQuery;
use eframe::egui::{self, Key, RichText, Role};

use super::{Action, label_icon};
use crate::search_view::TabSearch;
use crate::theme;

/// Typing waits this long before the search starts.
pub const DEBOUNCE: Duration = Duration::from_millis(150);
/// Hint text of the field (also its accessible name).
pub const HINT: &str = "Search text…";
/// Accessible name of the field.
pub const FIELD: &str = "Search";
/// Accessible names of the buttons.
pub const CASE: &str = "Match case";
/// See [`CASE`].
pub const WORD: &str = "Whole word";
/// See [`CASE`].
pub const REGEX: &str = "Regular expression";
/// See [`CASE`].
pub const SCOPE: &str = "Search scope";
/// See [`CASE`].
pub const NEXT: &str = "Next match";
/// See [`CASE`].
pub const PREV: &str = "Previous match";
/// See [`CASE`].
pub const CLEAR: &str = "Clear search";
/// See [`CASE`].
pub const HISTORY: &str = "Search history";
/// See [`CASE`].
pub const RESULTS: &str = "Results";
/// Tooltip of the features that arrive with step 13.
pub const COMING: &str = "Coming in v1";

/// The bar's state.
#[derive(Debug)]
pub struct SearchBar {
    /// The text in the field.
    pub text: String,
    /// `Aa`.
    pub case_sensitive: bool,
    /// `ab|`.
    pub whole_word: bool,
    /// The query tabs run (the text lags behind typing by [`DEBOUNCE`]).
    pub settled: SearchQuery,
    edited_at: Option<Instant>,
    /// The last edit of the text (kept after it settles; for timings).
    pub last_edit: Option<Instant>,
    /// Focus the field and select its text on the next frame.
    pub focus: bool,
    /// The field had the keyboard focus last frame.
    pub has_focus: bool,
    /// The results panel is expanded.
    pub results_open: bool,
}

impl Default for SearchBar {
    fn default() -> Self {
        SearchBar {
            text: String::new(),
            case_sensitive: false,
            whole_word: false,
            settled: SearchQuery::default(),
            edited_at: None,
            last_edit: None,
            focus: false,
            has_focus: false,
            results_open: true,
        }
    }
}

impl SearchBar {
    /// The query of the current text and toggles.
    pub fn query(&self) -> SearchQuery {
        SearchQuery {
            text: self.text.clone(),
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            ..SearchQuery::default()
        }
    }

    /// Settles the query now (toggles, Enter, history, clearing).
    pub fn settle(&mut self) {
        self.edited_at = None;
        self.settled = self.query();
    }

    /// Settles the text once typing paused for [`DEBOUNCE`]; returns how long to wait
    /// otherwise.
    pub fn tick(&mut self) -> Option<Duration> {
        let at = self.edited_at?;
        let waited = at.elapsed();
        if waited >= DEBOUNCE {
            self.settle();
            None
        } else {
            Some(DEBOUNCE - waited)
        }
    }

    /// Replaces the text (history pick) and searches right away.
    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.settle();
    }
}

/// Draws the bar for the active tab's `search`.
pub fn show(
    ui: &mut egui::Ui,
    bar: &mut SearchBar,
    search: Option<&TabSearch>,
    history: &[String],
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let hist = ui
            .add_enabled(!history.is_empty(), egui::Button::new("▾").frame(false))
            .on_hover_text("Recent searches");
        label_icon(&hist, Role::Button, HISTORY);
        egui::Popup::menu(&hist).show(|ui| {
            for h in history {
                if ui.button(h).clicked() {
                    actions.push(Action::SearchHistory(h.clone()));
                    ui.close();
                }
            }
        });

        let id = egui::Id::unique("search_field");
        let field = egui::TextEdit::singleline(&mut bar.text)
            .id(id)
            .hint_text(HINT)
            .desired_width(320.0)
            .return_key(None);
        let response = ui.add(field);
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::text_edit(true, &bar.text, &bar.text, HINT);
            info.label = Some(FIELD.to_owned());
            info
        });
        if bar.focus {
            bar.focus = false;
            response.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                let all = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(bar.text.chars().count()),
                );
                state.cursor.set_char_range(Some(all));
                state.store(ui.ctx(), id);
            }
        }
        if response.changed() {
            bar.last_edit = Some(Instant::now());
            if bar.text.is_empty() {
                bar.settle();
            } else {
                bar.edited_at = Some(Instant::now());
            }
        }
        bar.has_focus = response.has_focus();
        if response.has_focus() {
            let (enter, shift) = ui.input_mut(|i| {
                let shift = i.consume_key(egui::Modifiers::SHIFT, Key::Enter);
                (
                    shift || i.consume_key(egui::Modifiers::NONE, Key::Enter),
                    shift,
                )
            });
            if enter {
                if bar.settled != bar.query() {
                    bar.settle();
                }
                actions.push(if shift {
                    Action::SearchPrev
                } else {
                    Action::SearchNext
                });
            }
        }
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            if bar.text.is_empty() {
                actions.push(Action::CloseSearch);
            } else {
                bar.text.clear();
                bar.settle();
                response.request_focus();
            }
        }

        let before = (bar.case_sensitive, bar.whole_word);
        toggle(ui, &mut bar.case_sensitive, "Aa", CASE);
        toggle(ui, &mut bar.whole_word, "ab|", WORD);
        if before != (bar.case_sensitive, bar.whole_word) {
            bar.settle();
        }
        let regex = ui
            .add_enabled(false, egui::Button::new(RichText::new(".*").monospace()))
            .on_disabled_hover_text(COMING);
        label_icon(&regex, Role::Button, REGEX);
        let scope = ui
            .add_enabled(false, egui::Button::new("All ▾"))
            .on_disabled_hover_text(format!("Keys only / values only: {COMING}"));
        label_icon(&scope, Role::Button, SCOPE);

        ui.add_space(8.0);
        let count = search.map(TabSearch::count_text).unwrap_or_default();
        let error = search.is_some_and(|s| s.error.is_some());
        let color = if error {
            theme::error_color(ui.visuals().dark_mode)
        } else {
            ui.visuals().text_color()
        };
        ui.label(RichText::new(count).color(color));
        let has = search.is_some_and(|s| s.count() > 0);
        if search.is_some_and(|s| s.is_stepping()) {
            ui.spinner();
        }
        for (text, name, action) in [
            ("‹", PREV, Action::SearchPrev),
            ("›", NEXT, Action::SearchNext),
        ] {
            let b = ui
                .add_enabled(has, egui::Button::new(text).small())
                .on_hover_text(if name == NEXT {
                    "Next match (Enter, F3)"
                } else {
                    "Previous match (Shift+Enter, Shift+F3)"
                });
            label_icon(&b, Role::Button, name);
            if b.clicked() {
                actions.push(action);
            }
        }
        let clear = ui
            .add_enabled(!bar.text.is_empty(), egui::Button::new("×").small())
            .on_hover_text("Clear (Esc)");
        label_icon(&clear, Role::Button, CLEAR);
        if clear.clicked() {
            bar.text.clear();
            bar.settle();
        }
        if let Some((toast, _)) = search.and_then(|s| s.toast.as_ref()) {
            ui.label(RichText::new(toast).color(theme::accent(ui)).italics());
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let r = ui
                .selectable_label(bar.results_open, RESULTS)
                .on_hover_text(format!(
                    "Show or hide the results panel ({})",
                    if ui.ctx().os() == egui::os::OperatingSystem::Mac {
                        "⌘⇧F"
                    } else {
                        "Ctrl+Shift+F"
                    }
                ));
            if r.clicked() {
                actions.push(Action::ToggleResults);
            }
        });
    });
}

fn toggle(ui: &mut egui::Ui, value: &mut bool, text: &str, name: &str) {
    let r = ui
        .add(egui::Button::selectable(
            *value,
            RichText::new(text).monospace(),
        ))
        .on_hover_text(name);
    r.widget_info(|| egui::WidgetInfo::selected(Role::CheckBox, true, *value, name));
    if r.clicked() {
        *value = !*value;
    }
}
