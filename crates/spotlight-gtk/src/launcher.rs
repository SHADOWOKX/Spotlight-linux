use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use spotlight_core::{
    ProviderId, QueryGeneration, SearchResult,
    search::SearchUpdate,
    settings::{Settings, SettingsStore, XdgPaths},
};

use crate::{
    actions::{self, ActionOutcome},
    activation_trace::ActivationEvent,
    backend::Backend,
    platform::global_shortcut::{ShortcutEvent, ShortcutService},
    preferences,
    result_row::ResultRow,
    style,
};

#[derive(Clone, Debug, Default)]
pub struct ShortcutStatus {
    pub portal_version: Option<u32>,
    pub trigger_description: Option<String>,
    pub error: Option<String>,
    pub connection: Option<String>,
    pub session: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct DiagnosticsSnapshot {
    pub application_count: usize,
    pub files_seen: usize,
    pub invalid_entries: usize,
    pub shortcut: ShortcutStatus,
    pub latency: Option<spotlight_core::performance::LatencySummary>,
    pub ui_update_micros: u128,
    pub maximum_ui_update_micros: u128,
    pub allocated_rows: u64,
}

struct LauncherState {
    backend: Option<Backend>,
    generation: QueryGeneration,
    batches: BTreeMap<ProviderId, Vec<SearchResult>>,
    results: Vec<SearchResult>,
    selected: usize,
}

impl Default for LauncherState {
    fn default() -> Self {
        Self {
            backend: None,
            generation: QueryGeneration(0),
            batches: BTreeMap::new(),
            results: vec![],
            selected: 0,
        }
    }
}

pub struct Launcher {
    window: adw::ApplicationWindow,
    entry: gtk::SearchEntry,
    list: gtk::ListBox,
    rows: RefCell<Vec<ResultRow>>,
    scroller: gtk::ScrolledWindow,
    results_area: gtk::Box,
    palette_surface: gtk::Box,
    palette_frame: gtk::Box,
    input_bounds: Cell<Option<(i32, i32, i32, i32)>>,
    section_heading: gtk::Label,
    open_button: gtk::Button,
    actions_button: gtk::Button,
    empty_state: gtk::Label,
    footer: gtk::Label,
    toast_overlay: adw::ToastOverlay,
    action_popover: RefCell<Option<gtk::Popover>>,
    preferences_dialog: RefCell<Option<adw::PreferencesDialog>>,
    state: RefCell<LauncherState>,
    settings: Rc<RefCell<Settings>>,
    settings_store: SettingsStore,
    paths: XdgPaths,
    shortcut_service: ShortcutService,
    shortcut_status: Rc<RefCell<ShortcutStatus>>,
    dynamic_css: gtk::CssProvider,
    pub(crate) shortcut_notice: gio::SimpleAction,
    ui_metrics: Cell<UiMetrics>,
    show_started: Cell<Option<Instant>>,
    paint_requested: Cell<Option<Instant>>,
    focus_dismissal: FocusDismissal,
}

#[derive(Clone, Copy, Debug, Default)]
struct UiMetrics {
    shortcut_requests: u64,
    token_requests: u64,
    shows: u64,
    hides: u64,
    focused: u64,
    unfocused: u64,
    last_hide_reason: &'static str,
    show_to_focus_us: Option<u128>,
    show_to_paint_us: Option<u128>,
    last_render_us: u128,
    maximum_render_us: u128,
    rows_created: u64,
}

/// Ignore an old inactive notification until this presentation gains focus.
/// This is event-driven; no timeout, debounce, or dependency on key releases.
#[derive(Default)]
struct FocusDismissal(Cell<bool>);

impl FocusDismissal {
    fn reset(&self) {
        self.0.set(false);
    }

    fn observe(&self, visible: bool, active: bool) -> bool {
        if !visible {
            self.reset();
            false
        } else if active {
            self.0.set(true);
            false
        } else {
            self.0.replace(false)
        }
    }
}

impl Launcher {
    pub fn new(
        application: &adw::Application,
        settings: Settings,
        settings_store: SettingsStore,
        paths: XdgPaths,
        start_hidden: bool,
    ) -> Rc<Self> {
        let settings = Rc::new(RefCell::new(settings));
        let window = adw::ApplicationWindow::builder()
            .application(application)
            .title("Spotlight Linux")
            .default_height(-1)
            // Override libadwaita's 200px minimum; content still determines
            // the accessible minimum height at the user's font scale.
            .height_request(0)
            .resizable(false)
            .decorated(false)
            .hide_on_close(true)
            .css_classes(["launcher-window"])
            .build();

        let toast_overlay = adw::ToastOverlay::new();
        let surface = gtk::Box::new(gtk::Orientation::Vertical, 0);
        surface.add_css_class("launcher-surface");

        let search_area = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        search_area.add_css_class("search-area");
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Search apps or calculate…")
            .hexpand(true)
            .search_delay(0)
            .build();
        entry.add_css_class("launcher-search");
        search_area.append(&entry);
        let scope = gtk::Label::new(Some("Search"));
        scope.add_css_class("search-scope");
        scope.set_valign(gtk::Align::Center);
        search_area.append(&scope);
        surface.append(&search_area);
        let results_area = gtk::Box::new(gtk::Orientation::Vertical, 0);
        surface.append(&results_area);

        let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        separator.add_css_class("results-separator");
        results_area.append(&separator);

        let section_heading = gtk::Label::new(Some("SUGGESTED"));
        section_heading.set_xalign(0.0);
        section_heading.add_css_class("section-heading");
        results_area.append(&section_heading);

        let results_overlay = gtk::Overlay::new();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(156)
            .max_content_height(350)
            .propagate_natural_height(true)
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .activate_on_single_click(false)
            .build();
        list.add_css_class("results-list");
        scroller.set_child(Some(&list));
        results_overlay.set_child(Some(&scroller));

        let empty_state = gtk::Label::new(Some("Indexing applications…"));
        empty_state.add_css_class("empty-state");
        empty_state.set_halign(gtk::Align::Center);
        empty_state.set_valign(gtk::Align::Center);
        results_overlay.add_overlay(&empty_state);
        results_area.append(&results_overlay);

        let footer_bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        footer_bar.add_css_class("palette-footer");
        let settings_button = footer_button("Settings", "Ctrl+,");
        settings_button.set_tooltip_text(Some("Settings · Ctrl+,"));
        settings_button.set_action_name(Some("app.settings"));
        footer_bar.append(&settings_button);
        let footer = gtk::Label::new(Some("Local search"));
        footer.add_css_class("launcher-footer");
        footer.set_xalign(0.0);
        footer.set_hexpand(true);
        footer.set_ellipsize(gtk::pango::EllipsizeMode::End);
        footer_bar.append(&footer);
        let open_button = footer_button("Open", "↵");
        footer_bar.append(&open_button);
        let actions_button = footer_button("Actions", "Tab");
        footer_bar.append(&actions_button);
        results_area.append(&footer_bar);

        // Keep the native footprint stable: Wayland compositors retain the
        // top edge on resize, rather than re-centering an expanded search.
        let palette_frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        // Anchor the search entry at the expanded panel's top in both states.
        // Only the results below it change visibility while typing.
        surface.set_valign(gtk::Align::Start);
        surface.set_vexpand(true);
        palette_frame.append(&surface);
        toast_overlay.set_child(Some(&palette_frame));
        window.set_content(Some(&toast_overlay));

        let dynamic_css = style::install();
        style::apply(&window, &dynamic_css, &settings.borrow());
        let (shortcut_service, shortcut_events) =
            ShortcutService::start(settings.borrow().keyboard.launcher_shortcut.clone());
        let shortcut_status = Rc::new(RefCell::new(ShortcutStatus::default()));
        let launcher = Rc::new(Self {
            window,
            entry,
            list,
            rows: RefCell::new(Vec::new()),
            scroller,
            results_area,
            palette_surface: surface,
            palette_frame,
            input_bounds: Cell::new(None),
            section_heading,
            open_button: open_button.clone(),
            actions_button: actions_button.clone(),
            empty_state,
            footer,
            toast_overlay,
            action_popover: RefCell::new(None),
            preferences_dialog: RefCell::new(None),
            state: RefCell::new(LauncherState::default()),
            settings,
            settings_store,
            paths,
            shortcut_service,
            shortcut_status,
            dynamic_css,
            shortcut_notice: gio::SimpleAction::new_stateful(
                "shortcut-status",
                None,
                &"Connecting to desktop…".to_variant(),
            ),
            ui_metrics: Cell::new(UiMetrics::default()),
            show_started: Cell::new(None),
            paint_requested: Cell::new(None),
            focus_dismissal: FocusDismissal::default(),
        });

        launcher.connect_signals();
        let weak = Rc::downgrade(&launcher);
        open_button.connect_clicked(move |_| {
            if let Some(launcher) = weak.upgrade() {
                launcher.execute_selected();
            }
        });
        let weak = Rc::downgrade(&launcher);
        actions_button.connect_clicked(move |_| {
            if let Some(launcher) = weak.upgrade() {
                launcher.show_action_menu();
            }
        });
        launcher.apply_settings();
        launcher.start_backend();
        launcher.consume_shortcut_events(shortcut_events);
        if !start_hidden {
            launcher.present();
        }
        launcher
    }

    fn connect_signals(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        adw::StyleManager::default().connect_dark_notify(move |_| {
            let weak = weak.clone();
            glib::idle_add_local_once(move || {
                if let Some(launcher) = weak.upgrade() {
                    launcher.apply_settings();
                }
            });
        });
        if let Some(settings) = gtk::Settings::default() {
            let weak = Rc::downgrade(self);
            settings.connect_gtk_enable_animations_notify(move |_| {
                if let Some(launcher) = weak.upgrade() {
                    launcher.apply_settings();
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.entry.connect_search_changed(move |entry| {
            if let Some(launcher) = weak.upgrade() {
                launcher.submit(entry.text().as_str());
            }
        });

        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            if let Some(launcher) = weak.upgrade() {
                launcher.execute_index(row.index().max(0) as usize, None);
            }
        });

        let weak = Rc::downgrade(self);
        self.list.connect_row_selected(move |_, row| {
            if let (Some(launcher), Some(row)) = (weak.upgrade(), row) {
                launcher.state.borrow_mut().selected = row.index().max(0) as usize;
                launcher.update_primary_hint();
            }
        });

        let controller = gtk::EventControllerKey::new();
        controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        controller.connect_key_pressed(move |_, key, _, modifiers| {
            weak.upgrade()
                .map_or(glib::Propagation::Proceed, |launcher| {
                    launcher.handle_key(key, modifiers)
                })
        });
        self.window.add_controller(controller);

        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            if let Some(launcher) = weak.upgrade() {
                launcher.dismiss("window-close");
            }
            glib::Propagation::Stop
        });

        let weak = Rc::downgrade(self);
        self.window.connect_is_active_notify(move |window| {
            let Some(launcher) = weak.upgrade() else {
                return;
            };
            let mut metrics = launcher.ui_metrics.get();
            if window.is_active() {
                metrics.focused += 1;
                if metrics.show_to_focus_us.is_none() {
                    metrics.show_to_focus_us = launcher
                        .show_started
                        .get()
                        .map(|start| start.elapsed().as_micros());
                }
            } else {
                metrics.unfocused += 1;
            }
            launcher.ui_metrics.set(metrics);
            launcher.trace_window("active-changed");
            let lost_focus = launcher
                .focus_dismissal
                .observe(window.is_visible(), window.is_active());
            // Settings and action popovers own keyboard focus; they must remain
            // usable when their native portal confirmation takes focus away.
            if lost_focus
                && launcher.preferences_dialog.borrow().is_none()
                && launcher.action_popover.borrow().is_none()
            {
                launcher.dismiss("focus-loss");
            }
        });

        let weak = Rc::downgrade(self);
        self.window.connect_map(move |_| {
            if let Some(launcher) = weak.upgrade() {
                launcher.input_bounds.set(None);
                launcher.trace_window("mapped");
            }
        });
        let weak = Rc::downgrade(self);
        self.window.connect_unmap(move |_| {
            if let Some(launcher) = weak.upgrade() {
                launcher.trace_window("unmapped");
            }
        });
        let weak = Rc::downgrade(self);
        self.window.connect_focus_widget_notify(move |_| {
            if let Some(launcher) = weak.upgrade() {
                launcher.trace_window("focus-widget-changed");
            }
        });

        let weak = Rc::downgrade(self);
        self.window.connect_realize(move |window| {
            if let Some(clock) = window.frame_clock() {
                let weak = weak.clone();
                clock.connect_after_paint(move |_| {
                    if let Some(launcher) = weak.upgrade() {
                        launcher.update_input_region();
                        let Some(start) = launcher.paint_requested.take() else {
                            return;
                        };
                        let mut metrics = launcher.ui_metrics.get();
                        metrics.show_to_paint_us = Some(start.elapsed().as_micros());
                        launcher.ui_metrics.set(metrics);
                        launcher.trace_window("first-paint-submitted");
                    }
                });
            }
        });
    }

    fn start_backend(self: &Rc<Self>) {
        let receiver = Backend::initialize(
            self.paths.history_file(),
            self.settings.borrow().privacy.usage_history,
        );
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let Ok(mut backend) = receiver.recv().await else {
                if let Some(launcher) = weak.upgrade() {
                    launcher
                        .empty_state
                        .set_label("Application index failed to start");
                }
                return;
            };
            let updates = backend.take_updates();
            if let Some(launcher) = weak.upgrade() {
                let count = backend.application_count();
                if let Some(warning) = backend.warning() {
                    tracing::warn!(warning, "history started in degraded mode");
                }
                launcher.state.borrow_mut().backend = Some(backend);
                launcher
                    .footer
                    .set_label(&format!("{count} applications · Local"));
                launcher.submit(launcher.entry.text().as_str());
            } else {
                return;
            }

            while let Ok(update) = updates.recv().await {
                let Some(launcher) = weak.upgrade() else {
                    break;
                };
                launcher.handle_update(update);
            }
        });
    }

    fn consume_shortcut_events(self: &Rc<Self>, events: async_channel::Receiver<ShortcutEvent>) {
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            while let Ok(event) = events.recv().await {
                let Some(launcher) = weak.upgrade() else {
                    break;
                };
                match event {
                    ShortcutEvent::Connecting => {
                        *launcher.shortcut_status.borrow_mut() = ShortcutStatus::default();
                    }
                    ShortcutEvent::Ready {
                        portal_version,
                        trigger_description,
                        connection,
                        session,
                        awaiting_approval,
                    } => {
                        *launcher.shortcut_status.borrow_mut() = ShortcutStatus {
                            portal_version: Some(portal_version),
                            trigger_description,
                            error: awaiting_approval.then(|| "Desktop identity registered. Waiting for shortcut authorization in the native desktop dialog.".to_owned()),
                            connection: Some(connection),
                            session: Some(session),
                        };
                    }
                    ShortcutEvent::Changed {
                        trigger_description,
                    } => {
                        launcher.shortcut_status.borrow_mut().trigger_description =
                            trigger_description;
                        launcher.shortcut_status.borrow_mut().error = None;
                    }
                    ShortcutEvent::Activated { activation_token } => {
                        launcher.toggle_from_shortcut(activation_token.as_deref());
                        // The summon path only changes visibility/focus. Do not
                        // reread desktop settings or rebuild diagnostics here.
                        continue;
                    }
                    ShortcutEvent::Failed { message } => {
                        tracing::warn!(%message, "global shortcut unavailable");
                        *launcher.shortcut_status.borrow_mut() = ShortcutStatus {
                            error: Some(message),
                            ..Default::default()
                        };
                    }
                    ShortcutEvent::Notice { message } => {
                        launcher.shortcut_status.borrow_mut().error = Some(message);
                    }
                }
                launcher.refresh_shortcut_notice();
            }
        });
    }

    fn submit(&self, text: &str) {
        self.close_action_menu();
        self.update_results_visibility();
        let maximum_results = self.settings.borrow().search.maximum_results;
        let mut state = self.state.borrow_mut();
        let Some(backend) = state.backend.as_ref() else {
            self.empty_state.set_label("Indexing applications…");
            self.empty_state.set_visible(true);
            return;
        };
        backend.configure_search(&self.settings.borrow().search);
        if (!self.settings.borrow().search.applications_enabled
            && !self.settings.borrow().search.calculator_enabled)
            || (!self.settings.borrow().search.show_suggestions && text.trim().is_empty())
        {
            state.generation = backend.engine().cancel();
            state.batches.clear();
            state.results.clear();
            state.selected = 0;
            drop(state);
            self.render_results();
            self.footer.set_label("Search disabled · Ctrl+, Settings");
            return;
        }
        let generation = backend.engine().submit(text, maximum_results);
        state.generation = generation;
        state.batches.clear();
        state.results.clear();
        state.selected = 0;
        drop(state);
        self.render_results();
    }

    fn handle_update(&self, update: SearchUpdate) {
        if update.generation() != self.state.borrow().generation {
            return;
        }
        match update {
            SearchUpdate::Started { .. } => {}
            SearchUpdate::ProviderBatch {
                provider, results, ..
            } => {
                let maximum_results = self.settings.borrow().search.maximum_results;
                let mut state = self.state.borrow_mut();
                state.batches.insert(provider, results);
                let mut merged = state
                    .batches
                    .values()
                    .flat_map(|batch| batch.iter().cloned())
                    .collect::<Vec<_>>();
                merged.sort_by(SearchResult::stable_cmp);
                merged.truncate(maximum_results);
                state.results = merged;
                state.selected = state.selected.min(state.results.len().saturating_sub(1));
                drop(state);
                self.render_results();
            }
            SearchUpdate::ProviderFailed {
                provider, message, ..
            } => {
                tracing::warn!(provider = %provider.0, %message, "search provider failed");
                self.show_toast("One search provider could not respond");
            }
            SearchUpdate::Finished { elapsed, .. } => self.update_footer(elapsed),
        }
    }

    fn update_results_visibility(&self) {
        let visible =
            self.settings.borrow().search.show_suggestions || !self.entry.text().trim().is_empty();
        if self.results_area.is_visible() != visible {
            self.results_area.set_visible(visible);
        }
    }

    fn update_input_region(&self) {
        let Some(bounds) = self.palette_surface.compute_bounds(&self.window) else {
            return;
        };
        let Some(surface) = self.window.surface() else {
            return;
        };
        let (offset_x, offset_y) = self.window.surface_transform();
        let rect = (
            (f64::from(bounds.x()) + offset_x).floor() as i32,
            (f64::from(bounds.y()) + offset_y).floor() as i32,
            bounds.width().ceil() as i32,
            bounds.height().ceil() as i32,
        );
        if self.input_bounds.replace(Some(rect)) != Some(rect) {
            let region = gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(
                rect.0, rect.1, rect.2, rect.3,
            ));
            surface.set_input_region(Some(&region));
        }
    }

    fn render_results(&self) {
        self.update_results_visibility();
        let started = Instant::now();
        let (results, selected) = {
            let state = self.state.borrow();
            (state.results.clone(), state.selected)
        };
        let appearance = self.settings.borrow().appearance.clone();
        let mut rows = self.rows.borrow_mut();
        let previous_count = rows.len();
        while rows.len() < results.len() {
            let row = ResultRow::new();
            self.list.append(&row.widget);
            rows.push(row);
        }
        for (index, row) in rows.iter().enumerate() {
            if let Some(result) = results.get(index) {
                row.update(result, &appearance);
                row.widget.set_visible(true);
            } else {
                row.widget.set_visible(false);
            }
        }
        let empty = results.is_empty();
        self.open_button.set_sensitive(!empty);
        self.actions_button.set_sensitive(!empty);
        self.empty_state.set_visible(empty);
        self.empty_state
            .set_label(if !self.settings.borrow().search.applications_enabled {
                "Application search is off\nEnable it in Settings → Search"
            } else if self.state.borrow().backend.is_none() {
                "Indexing applications…"
            } else if self.entry.text().is_empty() {
                "No applications are available"
            } else {
                "No matching applications"
            });
        if !empty && let Some(row) = self.list.row_at_index(selected as i32) {
            self.list.select_row(Some(&row));
        }
        self.section_heading
            .set_label(if self.entry.text().is_empty() {
                "SUGGESTED"
            } else {
                "RESULTS"
            });
        let mut metrics = self.ui_metrics.get();
        metrics.rows_created += (rows.len() - previous_count) as u64;
        metrics.last_render_us = started.elapsed().as_micros();
        metrics.maximum_render_us = metrics.maximum_render_us.max(metrics.last_render_us);
        self.ui_metrics.set(metrics);
        self.update_primary_hint();
    }

    fn update_primary_hint(&self) {
        let state = self.state.borrow();
        let copy = state.results.get(state.selected).is_some_and(|result| {
            matches!(
                result.primary_action,
                spotlight_core::Action::CopyText { .. }
            )
        });
        if let Some(label) = self
            .open_button
            .child()
            .and_then(|child| child.first_child())
            .and_downcast::<gtk::Label>()
        {
            label.set_label(if copy { "Copy" } else { "Open" });
        }
    }

    fn update_footer(&self, elapsed: Duration) {
        let count = self
            .state
            .borrow()
            .backend
            .as_ref()
            .map_or(0, Backend::application_count);
        let micros = elapsed.as_micros();
        let latency = if micros < 1_000 {
            format!("{micros} µs")
        } else {
            format!("{:.1} ms", micros as f64 / 1_000.0)
        };
        self.footer
            .set_label(&if self.settings.borrow().search.show_latency {
                format!("{count} apps · {latency} search")
            } else {
                "↑↓ Navigate · Esc Close".into()
            });
        self.footer.set_tooltip_text(Some(&format!(
            "{count} indexed applications · Search {latency} · UI update {} µs",
            self.ui_metrics.get().last_render_us
        )));
    }

    fn handle_key(
        self: &Rc<Self>,
        key: gdk::Key,
        modifiers: gdk::ModifierType,
    ) -> glib::Propagation {
        if self.preferences_dialog.borrow().is_some() {
            return glib::Propagation::Proceed;
        }
        if self.action_popover.borrow().is_some() && key != gdk::Key::Escape {
            return glib::Propagation::Proceed;
        }
        let control = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        match key {
            gdk::Key::Escape => {
                if !self.close_action_menu() {
                    self.dismiss("escape");
                }
            }
            gdk::Key::Up | gdk::Key::KP_Up if !control => self.move_selection(-1),
            gdk::Key::Down | gdk::Key::KP_Down if !control => self.move_selection(1),
            gdk::Key::j if control => self.move_selection(1),
            gdk::Key::k if control => self.move_selection(-1),
            gdk::Key::Return | gdk::Key::KP_Enter if control => self.show_action_menu(),
            gdk::Key::Return | gdk::Key::KP_Enter => self.execute_selected(),
            gdk::Key::Tab | gdk::Key::ISO_Left_Tab => self.show_action_menu(),
            gdk::Key::comma if control => self.present_preferences(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    fn move_selection(&self, delta: isize) {
        let selected = {
            let mut state = self.state.borrow_mut();
            if state.results.is_empty() {
                return;
            }
            let length = state.results.len() as isize;
            state.selected = (state.selected as isize + delta).rem_euclid(length) as usize;
            state.selected
        };
        if let Some(row) = self.list.row_at_index(selected as i32) {
            self.list.select_row(Some(&row));
            row.grab_focus();
            self.entry.grab_focus();
        }
    }

    fn execute_selected(self: &Rc<Self>) {
        let index = self.state.borrow().selected;
        self.execute_index(index, None);
    }

    fn execute_index(
        self: &Rc<Self>,
        index: usize,
        override_action: Option<spotlight_core::Action>,
    ) {
        let result = self.state.borrow().results.get(index).cloned();
        let Some(result) = result else { return };
        let action = override_action.unwrap_or_else(|| result.primary_action.clone());
        match actions::execute(&action) {
            Ok(ActionOutcome::OpenSettings) => self.present_preferences(),
            Ok(outcome) => {
                if self.settings.borrow().privacy.usage_history
                    && matches!(
                        action,
                        spotlight_core::Action::LaunchDesktopEntry { .. }
                            | spotlight_core::Action::LaunchDesktopAction { .. }
                    )
                    && let Some(backend) = self.state.borrow().backend.as_ref()
                {
                    backend.record_launch(&result.id);
                }
                if outcome == ActionOutcome::Copied {
                    self.show_toast("Copied to clipboard");
                }
                if self.settings.borrow().general.close_after_action {
                    self.hide();
                }
            }
            Err(message) => self.show_toast(&message),
        }
    }

    fn show_action_menu(self: &Rc<Self>) {
        self.close_action_menu();
        let (index, result) = {
            let state = self.state.borrow();
            let Some(result) = state.results.get(state.selected).cloned() else {
                return;
            };
            (state.selected, result)
        };
        let Some(row) = self.list.row_at_index(index as i32) else {
            return;
        };
        let popover = gtk::Popover::new();
        popover.add_css_class("action-popover");
        popover.set_has_arrow(true);
        popover.set_autohide(true);
        popover.set_parent(&row);
        let actions_box = gtk::Box::new(gtk::Orientation::Vertical, 2);

        let mut actions = vec![("Open".to_owned(), result.primary_action.clone())];
        actions.extend(
            result
                .secondary_actions
                .iter()
                .map(|secondary| (secondary.title.clone(), secondary.action.clone())),
        );
        let mut first_button = None;
        for (title, action) in actions {
            let button = gtk::Button::with_label(&title);
            button.add_css_class("flat");
            button.add_css_class("action-button");
            button.set_halign(gtk::Align::Fill);
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(launcher) = weak.upgrade() {
                    launcher.close_action_menu();
                    launcher.execute_index(index, Some(action.clone()));
                }
            });
            if first_button.is_none() {
                first_button = Some(button.clone());
            }
            actions_box.append(&button);
        }
        popover.set_child(Some(&actions_box));
        let weak = Rc::downgrade(self);
        popover.connect_closed(move |popover| {
            if let Some(launcher) = weak.upgrade() {
                launcher.action_popover.borrow_mut().take();
            }
            popover.unparent();
        });
        *self.action_popover.borrow_mut() = Some(popover.clone());
        popover.popup();
        if let Some(button) = first_button {
            button.grab_focus();
        }
    }

    fn close_action_menu(&self) -> bool {
        let Some(popover) = self.action_popover.borrow_mut().take() else {
            return false;
        };
        popover.popdown();
        true
    }

    pub fn present(&self) {
        self.present_with_token(None);
    }

    pub fn toggle_from_shortcut(&self, activation_token: Option<&str>) {
        let mut metrics = self.ui_metrics.get();
        metrics.shortcut_requests += 1;
        metrics.token_requests += u64::from(activation_token.is_some());
        self.ui_metrics.set(metrics);
        self.trace_window("shortcut-toggle-request");
        if self.window.is_visible() && self.preferences_dialog.borrow().is_none() {
            self.dismiss("shortcut-toggle");
        } else {
            self.present_with_token(activation_token);
        }
    }

    fn present_with_token(&self, activation_token: Option<&str>) {
        if let Some(dialog) = self.preferences_dialog.borrow().clone() {
            // Non-resizable palettes host preferences in a separate native
            // window. An activation token must target the window being raised.
            if let Some(token) = activation_token
                && let Some(root) = dialog.root().and_downcast::<gtk::Window>()
            {
                root.set_startup_id(token);
            }
            self.present_detached_preferences(&dialog);
            self.trace_window("preferences-present-request");
            return;
        }
        let was_visible = self.window.is_visible();
        let already_focused = was_visible && self.window.is_active();
        let mut metrics = self.ui_metrics.get();
        metrics.shows += 1;
        if !already_focused {
            self.focus_dismissal.reset();
            metrics.show_to_focus_us = None;
            metrics.show_to_paint_us = None;
            let start = Instant::now();
            self.show_started.set(Some(start));
            self.paint_requested.set(Some(start));
        }
        self.ui_metrics.set(metrics);
        self.close_action_menu();
        // Set the fresh portal token BEFORE present/map. On Wayland GTK passes
        // this through xdg_activation_v1; no timestamp conversion or key hook.
        if let Some(token) = activation_token {
            self.window.set_startup_id(token);
            self.trace_window("activation-token-applied");
        }
        gtk::prelude::GtkWindowExt::set_focus(&self.window, Some(&self.entry));
        self.trace_window("present-request");
        self.window.present();
        self.entry.grab_focus();
        if !was_visible {
            self.entry.select_region(0, -1);
        }
        self.trace_window("present-returned");
    }

    /// Explicit CLI/action toggle. Portal Activated uses the same visibility
    /// policy with an activation token; Deactivated never changes visibility.
    pub fn toggle(&self) {
        if self.preferences_dialog.borrow().is_some() {
            self.present();
            return;
        }
        if self.window.is_visible() {
            self.dismiss("explicit-toggle");
            return;
        }
        self.present();
    }

    pub fn hide(&self) {
        self.dismiss("dismiss");
    }

    fn dismiss(&self, reason: &'static str) {
        self.trace_window(reason);
        self.focus_dismissal.reset();
        self.show_started.set(None);
        let mut metrics = self.ui_metrics.get();
        metrics.hides += 1;
        metrics.last_hide_reason = reason;
        self.ui_metrics.set(metrics);
        self.paint_requested.set(None);
        let dialog = self.preferences_dialog.borrow_mut().take();
        if let Some(dialog) = dialog {
            dialog.force_close();
        }
        self.close_action_menu();
        if !self.settings.borrow().general.remember_last_query {
            self.entry.set_text("");
        }
        self.window.set_visible(false);
    }

    pub fn present_preferences(self: &Rc<Self>) {
        if self.preferences_dialog.borrow().is_some() {
            self.present();
            return;
        }
        let dialog = preferences::build(self);
        // Track it before presenting: focus notifications can be synchronous.
        *self.preferences_dialog.borrow_mut() = Some(dialog.clone());
        let weak = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            if let Some(launcher) = weak.upgrade() {
                launcher.preferences_dialog.borrow_mut().take();
                if launcher.window.is_visible() {
                    launcher.entry.grab_focus();
                }
            }
        });
        self.present_detached_preferences(&dialog);
    }

    fn present_detached_preferences(&self, dialog: &adw::PreferencesDialog) {
        // A transient modal dialog is moved together with its parent by GNOME.
        // Settings must instead be independently movable for live preview.
        dialog.present(None::<&gtk::Widget>);
        if let Some(root) = dialog.root().and_downcast::<gtk::Window>() {
            root.set_application(self.window.application().as_ref());
            root.set_modal(false);
        }
    }

    pub(crate) fn settings(&self) -> Rc<RefCell<Settings>> {
        Rc::clone(&self.settings)
    }

    pub(crate) fn settings_store(&self) -> SettingsStore {
        self.settings_store.clone()
    }

    pub(crate) fn paths(&self) -> XdgPaths {
        self.paths.clone()
    }

    pub(crate) fn shortcut_service(&self) -> ShortcutService {
        self.shortcut_service.clone()
    }

    pub(crate) fn apply_settings(&self) {
        style::apply(&self.window, &self.dynamic_css, &self.settings.borrow());
        let appearance = self.settings.borrow().appearance.clone();
        let font_scale = gtk::Settings::default().map_or(1.0, |s| {
            (f64::from(s.gtk_xft_dpi()) / (96.0 * 1024.0)).max(1.0)
        });
        let height = (f64::from(
            (appearance.result_row_height.pixels() + 4) * appearance.visible_results as i32 + 14,
        ) * font_scale) as i32;
        self.scroller.set_min_content_height(0);
        self.scroller.set_max_content_height(height);
        self.scroller.set_min_content_height(height);
        // Measure the expanded layout even if suggestions are hidden. Both
        // states share its top edge and Settings keeps a stationary parent.
        let visible = self.results_area.is_visible();
        self.results_area.set_visible(true);
        let (_, natural_height, _, _) = self.palette_surface.measure(
            gtk::Orientation::Vertical,
            self.settings.borrow().appearance.window_width.pixels(),
        );
        self.palette_frame.set_height_request(natural_height);
        self.results_area.set_visible(visible);
        self.render_results();
    }

    pub(crate) fn apply_search_settings(&self) {
        if let Some(backend) = self.state.borrow().backend.as_ref() {
            backend.set_history_enabled(self.settings.borrow().privacy.usage_history);
        }
        self.submit(self.entry.text().as_str());
    }

    pub(crate) fn refresh_shortcut_notice(&self) {
        let shortcut = self.shortcut_status.borrow();
        let preferred = self.settings.borrow().keyboard.launcher_shortcut.clone();
        let current = shortcut
            .trigger_description
            .as_deref()
            .unwrap_or("Not assigned by the desktop");
        let mut message = format!(
            "Current shortcut: {current}\nPreferred: {}",
            crate::platform::shortcut_keys::label(&preferred)
        );
        if let Some(error) = shortcut.error.as_deref() {
            message.push_str(&format!("\n{error}"));
        }
        if let Some(conflict) = crate::platform::shortcut_keys::gnome_conflict(&preferred) {
            message.push_str(&format!("\n{conflict}"));
        }
        self.shortcut_notice.set_state(&message.to_variant());
        if shortcut.trigger_description.is_none()
            || crate::platform::shortcut_keys::gnome_conflict(&preferred).is_some()
        {
            self.show_toast("Shortcut needs attention — open Settings with Ctrl+, → Keyboard");
        }
    }

    fn search_focused(&self) -> bool {
        gtk::prelude::GtkWindowExt::focus(&self.window)
            .is_some_and(|widget| widget == self.entry || widget.is_ancestor(&self.entry))
    }

    fn trace_window(&self, event: &'static str) {
        self.shortcut_service.trace.record(ActivationEvent::Window {
            event,
            visible: self.window.is_visible(),
            mapped: self.window.is_mapped(),
            active: self.window.is_active(),
            search_focused: self.search_focused(),
        });
    }

    pub(crate) fn runtime_diagnostics(&self) -> String {
        let snapshot = self.diagnostics();
        format!(
            "application_id={}\npid={}\nvisible={}\nmapped={}\nwindow_active={}\nwindows={}\nsearch_focused={}\nresident=true\npreferred_shortcut={}\nportal_version={:?}\nportal_connection={:?}\nportal_session={:?}\ncurrent_shortcut={:?}\nshortcut_message={:?}\napplications={}\nshortcut_metrics={:?}\nwindow_metrics={:?}\nrenderer={}\n{}",
            spotlight_core::settings::APPLICATION_ID,
            std::process::id(),
            self.window.is_visible(),
            self.window.is_mapped(),
            self.window.is_active(),
            self.window
                .application()
                .map_or(0, |app| app.windows().len()),
            self.search_focused(),
            self.settings.borrow().keyboard.launcher_shortcut,
            snapshot.shortcut.portal_version,
            snapshot.shortcut.connection,
            snapshot.shortcut.session,
            snapshot.shortcut.trigger_description,
            snapshot.shortcut.error,
            snapshot.application_count,
            self.shortcut_service.metrics(),
            self.ui_metrics.get(),
            self.renderer_name(),
            self.shortcut_service.trace.snapshot(),
        )
    }

    pub(crate) fn renderer_name(&self) -> String {
        self.window.renderer().map_or_else(
            || "Not realized yet".into(),
            |renderer| renderer.type_().name().into(),
        )
    }

    pub(crate) fn diagnostics(&self) -> DiagnosticsSnapshot {
        let state = self.state.borrow();
        let (application_count, files_seen, invalid_entries, latency) = state
            .backend
            .as_ref()
            .map(|backend| {
                (
                    backend.application_count(),
                    backend.catalog_diagnostics().files_seen,
                    backend.catalog_diagnostics().invalid_entries,
                    Some(
                        backend
                            .engine()
                            .performance()
                            .summary_for(&ProviderId::from("applications")),
                    ),
                )
            })
            .unwrap_or((0, 0, 0, None));
        DiagnosticsSnapshot {
            application_count,
            files_seen,
            invalid_entries,
            shortcut: self.shortcut_status.borrow().clone(),
            latency,
            ui_update_micros: self.ui_metrics.get().last_render_us,
            maximum_ui_update_micros: self.ui_metrics.get().maximum_render_us,
            allocated_rows: self.ui_metrics.get().rows_created,
        }
    }

    pub(crate) fn clear_history(self: &Rc<Self>) {
        let Some(reply) = self
            .state
            .borrow()
            .backend
            .as_ref()
            .map(Backend::clear_history)
        else {
            self.show_toast("History is not ready yet; try again shortly");
            return;
        };
        let weak = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let result = reply.recv().await;
            if let Some(launcher) = weak.upgrade() {
                match result {
                    Ok(Ok(())) => {
                        launcher.show_toast("Usage history cleared");
                        launcher.submit(launcher.entry.text().as_str());
                    }
                    Ok(Err(error)) => {
                        launcher.show_toast(&format!("Could not clear usage history: {error}"))
                    }
                    Err(_) => launcher.show_toast(
                        "History worker stopped before confirming the clear; try again",
                    ),
                }
            }
        });
    }

    pub(crate) fn show_toast(&self, message: &str) {
        self.toast_overlay.add_toast(adw::Toast::new(message));
    }
}

fn footer_button(title: &str, shortcut: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("flat");
    button.add_css_class("footer-button");
    button.set_valign(gtk::Align::Center);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    content.append(&gtk::Label::new(Some(title)));
    let hint = gtk::Label::new(Some(shortcut));
    hint.add_css_class("shortcut-hint");
    content.append(&hint);
    button.set_child(Some(&content));
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_loss_dismissal_requires_focus_in_this_presentation() {
        let focus = FocusDismissal::default();
        for _ in 0..100 {
            focus.reset();
            assert!(!focus.observe(true, false)); // Late loss from previous window.
            assert!(!focus.observe(true, false)); // Still awaiting activation.
            assert!(!focus.observe(true, true)); // Compositor granted activation.
            assert!(focus.observe(true, false)); // Now an actual focus loss.
            assert!(!focus.observe(true, false));
            assert!(!focus.observe(false, true)); // Hidden focus is never armed.
            assert!(!focus.observe(true, false));
        }
    }

    #[test]
    fn gtk_palette_reuses_window_dismisses_and_leaves_dialog_keys_alone() {
        gtk::test_synced(|| {
            adw::init().unwrap();
            gtk::Settings::default()
                .unwrap()
                .set_gtk_enable_animations(false);
            let root = tempfile::tempdir().unwrap();
            let app = adw::Application::builder()
                .application_id("io.github.shadowokx.SpotlightLinux.Test")
                .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
                .build();
            app.register(None::<&gio::Cancellable>).unwrap();
            let hold = app.hold();
            let paths = XdgPaths {
                config_dir: root.path().join("config"),
                data_dir: root.path().join("data"),
                cache_dir: root.path().join("cache"),
                autostart_file: root.path().join("autostart/test.desktop"),
            };
            let launcher = Launcher::new(
                &app,
                Settings::default(),
                SettingsStore::new(paths.settings_file()),
                paths,
                true,
            );
            assert!(!launcher.window.is_visible());
            let settings_action = gio::SimpleAction::new("settings", None);
            let weak = Rc::downgrade(&launcher);
            settings_action.connect_activate(move |_, _| {
                if let Some(launcher) = weak.upgrade() {
                    launcher.present_preferences();
                }
            });
            app.add_action(&settings_action);
            for _ in 0..100 {
                launcher.toggle_from_shortcut(None);
                assert!(launcher.window.is_visible());
                assert!(launcher.search_focused());
                // A stale inactive notification cannot
                // dismiss a new presentation before it has received focus.
                launcher.window.notify("is-active");
                assert!(launcher.window.is_visible());
                launcher.toggle_from_shortcut(None);
                assert!(!launcher.window.is_visible());
                launcher.toggle_from_shortcut(None);
                assert!(launcher.window.is_visible());
                assert!(launcher.search_focused());
                launcher.entry.set_text("test query");
                assert_eq!(
                    launcher.handle_key(gdk::Key::Escape, gdk::ModifierType::empty()),
                    glib::Propagation::Stop
                );
                assert!(!launcher.window.is_visible());
                assert!(launcher.entry.text().is_empty());
                assert_eq!(app.windows().len(), 1);
            }
            launcher.present();
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            exercise_rendering(&launcher);
            launcher.present_preferences();
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            let dialog = launcher
                .preferences_dialog
                .borrow()
                .clone()
                .expect("Settings should open");
            dialog.set_visible_page_name("appearance");
            let settings_root = dialog.root().and_downcast::<gtk::Window>().unwrap();
            assert!(settings_root.transient_for().is_none());
            assert!(!settings_root.is_modal());
            assert_eq!(settings_root.application(), launcher.window.application());
            glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(120)));
            let settings_size = (settings_root.width(), settings_root.height());
            let parent_height = launcher.window.height();
            for show in [false, true, false] {
                launcher.settings.borrow_mut().search.show_suggestions = show;
                launcher.apply_search_settings();
                glib::MainContext::default()
                    .block_on(glib::timeout_future(Duration::from_millis(120)));
                assert_eq!(launcher.window.height(), parent_height);
                assert_eq!(
                    (settings_root.width(), settings_root.height()),
                    settings_size
                );
                assert!(settings_root.is_mapped());
            }
            if let Some(directory) = std::env::var_os("SPOTLIGHT_SCREENSHOTS") {
                let root = dialog.root().and_downcast::<gtk::Window>().unwrap();
                capture(
                    &root,
                    &std::path::Path::new(&directory).join("settings.png"),
                );
            }
            for key in [
                gdk::Key::Escape,
                gdk::Key::Tab,
                gdk::Key::Return,
                gdk::Key::Down,
            ] {
                assert_eq!(
                    launcher.handle_key(key, gdk::ModifierType::empty()),
                    glib::Propagation::Proceed
                );
            }
            launcher.present_preferences();
            assert_eq!(*launcher.preferences_dialog.borrow(), Some(dialog.clone()));
            assert!(settings_root.transient_for().is_none());
            assert!(!settings_root.is_modal());
            dialog.force_close();
            launcher.window.close();
            assert!(!launcher.window.is_visible());
            assert_eq!(app.windows().len(), 1);
            launcher.window.destroy();
            drop(launcher);
            drop(hold);
            app.quit();
        });
    }

    fn capture(window: &impl IsA<gtk::Window>, path: &std::path::Path) {
        // Test-only frame wait. Rendering the actual GTK tree, not a mockup.
        let window = window.as_ref();
        let paintable = gtk::WidgetPaintable::new(Some(window));
        window.present();
        window.queue_draw();
        glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(120)));
        assert!(
            window.is_mapped(),
            "capture must be mapped: {}",
            path.display()
        );
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(
            &snapshot,
            f64::from(window.width()),
            f64::from(window.height()),
        );
        let node = snapshot.to_node().unwrap_or_else(|| {
            panic!(
                "GTK must render {} ({}x{}, visible={})",
                path.display(),
                window.width(),
                window.height(),
                window.is_visible()
            )
        });
        window
            .renderer()
            .unwrap()
            .render_texture(&node, None)
            .save_to_png(path)
            .unwrap();
    }

    fn exercise_rendering(launcher: &Rc<Launcher>) {
        use spotlight_core::settings::{Theme, WindowStyle};
        let deadline = Instant::now() + Duration::from_secs(5);
        while launcher.state.borrow().backend.is_none() && Instant::now() < deadline {
            glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(10)));
        }
        assert!(launcher.state.borrow().backend.is_some());
        // The isolated bus intentionally has no desktop portal. Its expected
        // warning is unrelated to visual fixtures; dismiss it only in this test.
        launcher.toast_overlay.dismiss_all();
        let generation = launcher
            .state
            .borrow()
            .backend
            .as_ref()
            .unwrap()
            .engine()
            .cancel();
        launcher.state.borrow_mut().generation = generation;
        let fixtures = [
            (
                "Terminal",
                "Run commands and developer tools",
                "utilities-terminal",
            ),
            ("Files", "Browse your folders", "org.gnome.Nautilus"),
            ("Firefox", "Web browser", "firefox"),
            (
                "Text Editor",
                "A quiet space to write",
                "org.gnome.TextEditor",
            ),
            (
                "Calculator",
                "Calculate and convert",
                "org.gnome.Calculator",
            ),
            ("Settings", "Your desktop preferences", "org.gnome.Settings"),
            ("Calendar", "Keep track of your day", "org.gnome.Calendar"),
            ("Music", "Your music library", "org.gnome.Music"),
        ];
        launcher.state.borrow_mut().results = fixtures
            .into_iter()
            .enumerate()
            .map(|(index, (title, subtitle, icon))| SearchResult {
                id: format!("fixture:{index}"),
                title: title.into(),
                subtitle: Some(subtitle.into()),
                icon: spotlight_core::Icon::Themed(icon.into()),
                provider: "applications".into(),
                score: 100 - index as i64,
                primary_action: spotlight_core::Action::OpenSettings,
                secondary_actions: vec![],
                keywords: vec![],
                metadata: BTreeMap::new(),
            })
            .collect();
        launcher.render_results();
        let first = launcher.list.row_at_index(0).unwrap();
        let allocations = launcher.ui_metrics.get().rows_created;
        for _ in 0..100 {
            launcher.render_results();
        }
        assert_eq!(launcher.list.row_at_index(0).unwrap(), first);
        assert_eq!(launcher.ui_metrics.get().rows_created, allocations);
        assert_eq!(launcher.rows.borrow().len(), 8);
        launcher.move_selection(7);
        assert_eq!(launcher.state.borrow().selected, 7);
        launcher.move_selection(1);
        assert_eq!(launcher.state.borrow().selected, 0);
        if let Some(directory) = std::env::var_os("SPOTLIGHT_SCREENSHOTS") {
            for (name, palette) in [
                ("graphite", spotlight_core::settings::Palette::Graphite),
                ("midnight", spotlight_core::settings::Palette::Midnight),
                ("dusk", spotlight_core::settings::Palette::Dusk),
                ("forest", spotlight_core::settings::Palette::Forest),
            ] {
                for theme in [Theme::Dark, Theme::Light] {
                    launcher.settings.borrow_mut().appearance.palette = palette;
                    launcher.settings.borrow_mut().appearance.theme = theme;
                    launcher.settings.borrow_mut().appearance.window_style = WindowStyle::Normal;
                    launcher.apply_settings();
                    capture(
                        &launcher.window,
                        &std::path::Path::new(&directory).join(format!("{name}-{theme:?}.png")),
                    );
                }
            }
            launcher.settings.borrow_mut().appearance.palette =
                spotlight_core::settings::Palette::Native;
            for (name, theme, mode) in [
                ("dark.png", Theme::Dark, WindowStyle::Normal),
                ("light.png", Theme::Light, WindowStyle::Normal),
                ("glass.png", Theme::Dark, WindowStyle::Glass),
            ] {
                launcher.settings.borrow_mut().appearance.theme = theme;
                launcher.settings.borrow_mut().appearance.window_style = mode;
                launcher.apply_settings();
                capture(
                    &launcher.window,
                    &std::path::Path::new(&directory).join(name),
                );
            }
        }
        launcher.settings.borrow_mut().search.applications_enabled = false;
        launcher.apply_search_settings();
        assert!(launcher.state.borrow().results.is_empty());
        assert!(launcher.empty_state.label().contains("off"));
        launcher.entry.set_text("15% of 850");
        launcher.submit("15% of 850");
        glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(120)));
        assert_eq!(launcher.state.borrow().results.len(), 1);
        assert_eq!(launcher.state.borrow().results[0].title, "127.5");
        assert!(
            matches!(&launcher.state.borrow().results[0].primary_action, spotlight_core::Action::CopyText { text } if text == "127.5")
        );
        launcher.settings.borrow_mut().search.calculator_enabled = false;
        launcher.apply_search_settings();
        assert!(launcher.state.borrow().results.is_empty());
        launcher.settings.borrow_mut().search.calculator_enabled = true;
        launcher.entry.set_text("");
        launcher.settings.borrow_mut().search.applications_enabled = true;
        launcher.apply_search_settings();
        launcher.present();
        glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(120)));
        let expanded_height = launcher.window.height();
        launcher.settings.borrow_mut().search.show_suggestions = false;
        launcher.entry.set_text("");
        launcher.apply_search_settings();
        assert!(!launcher.results_area.is_visible());
        assert!(launcher.state.borrow().results.is_empty());
        glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(120)));
        assert_eq!(launcher.window.height(), expanded_height);
        assert!(launcher.palette_surface.height() < 120);
        let bounds = launcher
            .palette_surface
            .compute_bounds(&launcher.window)
            .unwrap();
        assert!(bounds.y().abs() <= 1.0);
        let entry_y = launcher.entry.compute_bounds(&launcher.window).unwrap().y();
        assert!(launcher.input_bounds.get().unwrap().3 < 120);
        if let Some(directory) = std::env::var_os("SPOTLIGHT_SCREENSHOTS") {
            capture(
                &launcher.window,
                &std::path::Path::new(&directory).join("search-only.png"),
            );
        }
        launcher.entry.set_text("terminal");
        launcher.submit("terminal");
        assert!(launcher.results_area.is_visible());
        glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(120)));
        assert_eq!(launcher.window.height(), expanded_height);
        let expanded = launcher
            .palette_surface
            .compute_bounds(&launcher.window)
            .unwrap();
        assert!((expanded.y() - bounds.y()).abs() <= 1.0);
        assert!(
            (launcher.entry.compute_bounds(&launcher.window).unwrap().y() - entry_y).abs() <= 1.0
        );
        let pending_generation = launcher.state.borrow().generation;
        launcher.entry.set_text("   ");
        launcher.submit("   ");
        assert_ne!(launcher.state.borrow().generation, pending_generation);
        assert!(!launcher.results_area.is_visible());
        glib::MainContext::default().block_on(glib::timeout_future(Duration::from_millis(120)));
        assert!(launcher.state.borrow().results.is_empty());
        assert!(!launcher.results_area.is_visible());
        assert!(
            (launcher.entry.compute_bounds(&launcher.window).unwrap().y() - entry_y).abs() <= 1.0
        );
        launcher.settings.borrow_mut().search.show_suggestions = true;
        launcher.entry.set_text("");
        launcher.apply_search_settings();
        assert!(launcher.results_area.is_visible());
    }
}
