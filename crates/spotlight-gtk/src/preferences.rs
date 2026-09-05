use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use adw::prelude::*;
use spotlight_core::settings::{
    Accent, AnimationPreference, CornerRadius, DEFAULT_LAUNCHER_SHORTCUT, Density, IconSize,
    Palette, ResultRowHeight, Settings, Theme, WindowStyle,
};

use crate::{
    launcher::Launcher,
    platform::{autostart, shortcut_keys},
};

pub fn build(launcher: &Rc<Launcher>) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::builder()
        .title("Spotlight Linux Settings")
        .search_enabled(true)
        .content_width(660)
        .content_height(680)
        .build();

    dialog.add(&general_page(launcher));
    dialog.add(&appearance_page(launcher));
    dialog.add(&search_page(launcher));
    dialog.add(&keyboard_page(launcher, &dialog));
    dialog.add(&privacy_page(launcher));
    dialog.add(&advanced_page(launcher));
    dialog.add(&about_page());
    let weak = Rc::downgrade(launcher);
    dialog.connect_closed(move |_| {
        // Flush a final opacity edit even if the dialog closes during its
        // short save-coalescing interval.
        if let Some(launcher) = weak.upgrade() {
            persist(&launcher, false);
        }
    });
    dialog
}

fn general_page(launcher: &Rc<Launcher>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder().title("Behavior").build();
    let settings = launcher.settings();

    let autostart = adw::SwitchRow::builder()
        .title("Launch at Login")
        .subtitle("Keep the shortcut ready without showing the launcher")
        .active(settings.borrow().general.launch_at_login)
        .build();
    let weak = Rc::downgrade(launcher);
    let updating = Cell::new(false);
    autostart.connect_active_notify(move |row| {
        if updating.get() {
            return;
        }
        let Some(launcher) = weak.upgrade() else {
            return;
        };
        let enabled = row.is_active();
        let old = launcher.settings().borrow().general.launch_at_login;
        match autostart::set_enabled(&launcher.paths().autostart_file, enabled) {
            Ok(()) => {
                launcher.settings().borrow_mut().general.launch_at_login = enabled;
                let saved = launcher
                    .settings_store()
                    .save(&launcher.settings().borrow());
                if let Err(error) = saved {
                    let rollback = autostart::set_enabled(&launcher.paths().autostart_file, old);
                    if rollback.is_ok() {
                        launcher.settings().borrow_mut().general.launch_at_login = old;
                    }
                    launcher.show_toast(&format!(
                        "Could not save launch-at-login preference: {error}"
                    ));
                }
            }
            Err(error) => {
                launcher.settings().borrow_mut().general.launch_at_login = old;
                launcher.show_toast(&error.to_string());
            }
        }
        updating.set(true);
        row.set_active(launcher.settings().borrow().general.launch_at_login);
        updating.set(false);
    });
    group.add(&autostart);

    let close_after = adw::SwitchRow::builder()
        .title("Close After Action")
        .subtitle("Hide the launcher after opening a result")
        .active(settings.borrow().general.close_after_action)
        .build();
    let weak = Rc::downgrade(launcher);
    close_after.connect_active_notify(move |row| {
        let Some(launcher) = weak.upgrade() else {
            return;
        };
        launcher.settings().borrow_mut().general.close_after_action = row.is_active();
        persist(&launcher, false);
    });
    group.add(&close_after);

    let remember_query = adw::SwitchRow::builder()
        .title("Remember Last Query")
        .subtitle("Keep search text when the launcher closes")
        .active(settings.borrow().general.remember_last_query)
        .build();
    let weak = Rc::downgrade(launcher);
    remember_query.connect_active_notify(move |row| {
        let Some(launcher) = weak.upgrade() else {
            return;
        };
        launcher.settings().borrow_mut().general.remember_last_query = row.is_active();
        persist(&launcher, false);
    });
    group.add(&remember_query);
    let quit = adw::ActionRow::builder().title("Background Launcher").subtitle("Closing the palette keeps its shortcut ready. Quit ends the shortcut session until you reopen Spotlight Linux.").build();
    let button = gtk::Button::with_label("Quit Spotlight");
    button.set_valign(gtk::Align::Center);
    button.set_action_name(Some("app.quit"));
    quit.add_suffix(&button);
    group.add(&quit);
    page.add(&group);
    page
}

fn appearance_page(launcher: &Rc<Launcher>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Appearance")
        .name("appearance")
        .icon_name("applications-graphics-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder()
        .title("Launcher")
        .description("Effects remain optional and never delay search input")
        .build();
    let settings = launcher.settings();
    let palette = adw::ComboRow::builder()
        .title("Color Palette")
        .subtitle("Original colors · pairs with Light, Dark and Glass")
        .model(&gtk::StringList::new(&[
            "Native", "Graphite", "Midnight", "Dusk", "Forest",
        ]))
        .selected(match settings.borrow().appearance.palette {
            Palette::Native => 0,
            Palette::Graphite => 1,
            Palette::Midnight => 2,
            Palette::Dusk => 3,
            Palette::Forest => 4,
        })
        .build();
    let weak = Rc::downgrade(launcher);
    palette.connect_selected_notify(move |row| {
        if let Some(launcher) = weak.upgrade() {
            launcher.settings().borrow_mut().appearance.palette = match row.selected() {
                1 => Palette::Graphite,
                2 => Palette::Midnight,
                3 => Palette::Dusk,
                4 => Palette::Forest,
                _ => Palette::Native,
            };
            persist(&launcher, true);
        }
    });
    group.add(&palette);
    for (title, search, min, max) in [
        ("Search Font Size", true, 16.0, 26.0),
        ("Result Font Size", false, 12.0, 18.0),
    ] {
        let row = adw::SpinRow::with_range(min, max, 1.0);
        row.set_title(title);
        row.set_subtitle("Pixels · system font scaling still applies");
        row.set_value(if search {
            settings.borrow().appearance.search_font_size
        } else {
            settings.borrow().appearance.result_font_size
        } as f64);
        let weak = Rc::downgrade(launcher);
        row.connect_value_notify(move |row| {
            if let Some(launcher) = weak.upgrade() {
                if search {
                    launcher.settings().borrow_mut().appearance.search_font_size =
                        row.value() as u32;
                } else {
                    launcher.settings().borrow_mut().appearance.result_font_size =
                        row.value() as u32;
                }
                persist(&launcher, true);
            }
        });
        group.add(&row);
    }
    let types = adw::SwitchRow::builder()
        .title("Show Result Type")
        .subtitle("Application or Calculator labels on the right")
        .active(settings.borrow().appearance.show_result_type)
        .build();
    let weak = Rc::downgrade(launcher);
    types.connect_active_notify(move |row| {
        if let Some(launcher) = weak.upgrade() {
            launcher.settings().borrow_mut().appearance.show_result_type = row.is_active();
            persist(&launcher, true);
        }
    });
    group.add(&types);

    let theme_model = gtk::StringList::new(&["Follow System", "Light", "Dark"]);
    let theme = adw::ComboRow::builder()
        .title("Theme")
        .model(&theme_model)
        .selected(match settings.borrow().appearance.theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        })
        .build();
    let weak = Rc::downgrade(launcher);
    theme.connect_selected_notify(move |row| {
        let Some(launcher) = weak.upgrade() else {
            return;
        };
        launcher.settings().borrow_mut().appearance.theme = match row.selected() {
            1 => Theme::Light,
            2 => Theme::Dark,
            _ => Theme::System,
        };
        persist(&launcher, true);
    });
    group.add(&theme);

    let style_model = gtk::StringList::new(&["Normal", "Glass", "Minimal"]);
    let window_style = adw::ComboRow::builder()
        .title("Window Style")
        .model(&style_model)
        .selected(match settings.borrow().appearance.window_style {
            WindowStyle::Normal => 0,
            WindowStyle::Glass => 1,
            WindowStyle::Minimal => 2,
        })
        .build();
    let weak = Rc::downgrade(launcher);
    window_style.connect_selected_notify(move |row| {
        let Some(launcher) = weak.upgrade() else {
            return;
        };
        launcher.settings().borrow_mut().appearance.window_style = match row.selected() {
            1 => WindowStyle::Glass,
            2 => WindowStyle::Minimal,
            _ => WindowStyle::Normal,
        };
        persist(&launcher, true);
    });
    group.add(&window_style);

    let transparency_row = adw::ActionRow::builder()
        .title("Glass Opacity")
        .subtitle("Higher means more opaque. Used only by Glass; blur is not simulated.")
        .build();
    let transparency = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.55, 1.0, 0.05);
    transparency.set_value(settings.borrow().appearance.transparency);
    transparency.set_draw_value(true);
    transparency.set_digits(2);
    transparency.set_size_request(190, -1);
    transparency.set_valign(gtk::Align::Center);
    let weak = Rc::downgrade(launcher);
    let pending_save: Rc<RefCell<Option<gtk::glib::SourceId>>> = Rc::default();
    transparency.connect_value_changed(move |scale| {
        let Some(launcher) = weak.upgrade() else {
            return;
        };
        launcher.settings().borrow_mut().appearance.transparency = scale.value();
        launcher.apply_settings();
        // Preview continuously; coalesce disk writes after a drag/key sequence.
        // Read the current snapshot at save time so another setting isn't lost.
        if let Some(source) = pending_save.borrow_mut().take() {
            source.remove();
        }
        let pending = Rc::clone(&pending_save);
        let weak = Rc::downgrade(&launcher);
        *pending_save.borrow_mut() = Some(gtk::glib::timeout_add_local_once(
            std::time::Duration::from_millis(180),
            move || {
                pending.borrow_mut().take();
                if let Some(launcher) = weak.upgrade() {
                    persist(&launcher, false);
                }
            },
        ));
    });
    transparency_row.add_suffix(&transparency);
    group.add(&transparency_row);

    let width_model = gtk::StringList::new(&["Compact", "Standard", "Wide"]);
    let width = adw::ComboRow::builder()
        .title("Window Width")
        .model(&width_model)
        .selected(match settings.borrow().appearance.window_width {
            spotlight_core::settings::WindowWidth::Compact => 0,
            spotlight_core::settings::WindowWidth::Standard => 1,
            spotlight_core::settings::WindowWidth::Wide => 2,
        })
        .build();
    let weak = Rc::downgrade(launcher);
    width.connect_selected_notify(move |row| {
        let Some(launcher) = weak.upgrade() else {
            return;
        };
        launcher.settings().borrow_mut().appearance.window_width = match row.selected() {
            0 => spotlight_core::settings::WindowWidth::Compact,
            2 => spotlight_core::settings::WindowWidth::Wide,
            _ => spotlight_core::settings::WindowWidth::Standard,
        };
        persist(&launcher, true);
    });
    group.add(&width);

    page.add(&group);
    let layout = adw::PreferencesGroup::builder()
        .title("Layout and Detail")
        .description("Changes apply immediately to the palette. Ctrl+, opens Settings at any time.")
        .build();
    let appearance = settings.borrow().appearance.clone();
    layout.add(&choice(
        launcher,
        "Accent",
        &["Graphite", "System", "Blue", "Violet", "Green"],
        match appearance.accent {
            Accent::Graphite => 0,
            Accent::System => 1,
            Accent::Blue => 2,
            Accent::Violet => 3,
            Accent::Green => 4,
        },
        true,
        |s, i| {
            s.appearance.accent = match i {
                1 => Accent::System,
                2 => Accent::Blue,
                3 => Accent::Violet,
                4 => Accent::Green,
                _ => Accent::Graphite,
            };
        },
    ));
    layout.add(&choice(
        launcher,
        "Density",
        &["Compact", "Comfortable"],
        u32::from(appearance.density == Density::Comfortable),
        true,
        |s, i| {
            s.appearance.density = if i == 0 {
                Density::Compact
            } else {
                Density::Comfortable
            };
        },
    ));
    layout.add(&choice(
        launcher,
        "Result Row Height",
        &["Compact", "Standard", "Spacious"],
        match appearance.result_row_height {
            ResultRowHeight::Compact => 0,
            ResultRowHeight::Standard => 1,
            ResultRowHeight::Spacious => 2,
        },
        true,
        |s, i| {
            s.appearance.result_row_height = match i {
                0 => ResultRowHeight::Compact,
                2 => ResultRowHeight::Spacious,
                _ => ResultRowHeight::Standard,
            };
        },
    ));
    layout.add(&choice(
        launcher,
        "Icons",
        &["Small", "Standard", "Large"],
        match appearance.icon_size {
            IconSize::Small => 0,
            IconSize::Standard => 1,
            IconSize::Large => 2,
        },
        true,
        |s, i| {
            s.appearance.icon_size = match i {
                0 => IconSize::Small,
                2 => IconSize::Large,
                _ => IconSize::Standard,
            };
        },
    ));
    layout.add(&choice(
        launcher,
        "Corners",
        &["Small", "Medium", "Large"],
        match appearance.corner_radius {
            CornerRadius::Small => 0,
            CornerRadius::Medium => 1,
            CornerRadius::Large => 2,
        },
        true,
        |s, i| {
            s.appearance.corner_radius = match i {
                0 => CornerRadius::Small,
                2 => CornerRadius::Large,
                _ => CornerRadius::Medium,
            };
        },
    ));
    layout.add(&choice(
        launcher,
        "Visible Results",
        &["4", "5", "6", "7", "8", "9", "10"],
        appearance.visible_results - 4,
        true,
        |s, i| {
            s.appearance.visible_results = i + 4;
        },
    ));
    let subtitles = adw::SwitchRow::builder()
        .title("Application Descriptions")
        .subtitle("Show useful details beneath application names")
        .active(appearance.show_subtitles)
        .build();
    let weak = Rc::downgrade(launcher);
    subtitles.connect_active_notify(move |row| {
        if let Some(launcher) = weak.upgrade() {
            launcher.settings().borrow_mut().appearance.show_subtitles = row.is_active();
            persist(&launcher, true);
        }
    });
    layout.add(&subtitles);
    page.add(&layout);

    let effects = adw::PreferencesGroup::builder()
        .title("Effects and Accessibility")
        .build();
    effects.add(&choice(
        launcher,
        "Animations",
        &["Full", "Reduced", "Off"],
        match appearance.animations {
            AnimationPreference::Full => 0,
            AnimationPreference::Reduced => 1,
            AnimationPreference::Off => 2,
        },
        true,
        |s, i| {
            s.appearance.animations = match i {
                1 => AnimationPreference::Reduced,
                2 => AnimationPreference::Off,
                _ => AnimationPreference::Full,
            };
        },
    ));
    effects.add(&detail_row("Background Blur", "Unavailable through standard GTK on this desktop. Glass uses lightweight translucency, never screenshot processing."));
    page.add(&effects);
    page
}

fn choice(
    launcher: &Rc<Launcher>,
    title: &str,
    values: &[&str],
    selected: u32,
    appearance: bool,
    update: impl Fn(&mut Settings, u32) + 'static,
) -> adw::ComboRow {
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(values))
        .selected(selected)
        .build();
    let weak = Rc::downgrade(launcher);
    row.connect_selected_notify(move |row| {
        if let Some(launcher) = weak.upgrade() {
            update(&mut launcher.settings().borrow_mut(), row.selected());
            persist(&launcher, appearance);
        }
    });
    row
}

fn search_page(launcher: &Rc<Launcher>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Search")
        .icon_name("system-search-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder()
        .title("Local Search")
        .description(
            "Indexed once, searched in memory. No network suggestions or keystroke uploads.",
        )
        .build();
    let settings = launcher.settings();
    let applications = adw::SwitchRow::builder()
        .title("Applications")
        .subtitle("Names, keywords and desktop actions")
        .active(settings.borrow().search.applications_enabled)
        .build();
    let weak = Rc::downgrade(launcher);
    applications.connect_active_notify(move |row| {
        if let Some(launcher) = weak.upgrade() {
            launcher.settings().borrow_mut().search.applications_enabled = row.is_active();
            persist(&launcher, false);
        }
    });
    group.add(&applications);
    let calculator = adw::SwitchRow::builder()
        .title("Calculator")
        .subtitle("Arithmetic, sqrt(144), 15% of 850 · Enter copies the answer")
        .active(settings.borrow().search.calculator_enabled)
        .build();
    let weak = Rc::downgrade(launcher);
    calculator.connect_active_notify(move |row| {
        if let Some(launcher) = weak.upgrade() {
            launcher.settings().borrow_mut().search.calculator_enabled = row.is_active();
            persist(&launcher, false);
        }
    });
    group.add(&calculator);
    let suggestions = adw::SwitchRow::builder()
        .title("Show Suggestions")
        .subtitle("Show apps before typing. Turn off for search only; Ctrl+, opens Settings.")
        .active(settings.borrow().search.show_suggestions)
        .build();
    let weak = Rc::downgrade(launcher);
    suggestions.connect_active_notify(move |row| {
        if let Some(launcher) = weak.upgrade() {
            launcher.settings().borrow_mut().search.show_suggestions = row.is_active();
            persist(&launcher, false);
        }
    });
    group.add(&suggestions);
    let limit = adw::SpinRow::with_range(1.0, 100.0, 1.0);
    limit.set_title("Maximum Results");
    limit.set_subtitle("Additional results scroll within the visible list");
    limit.set_value(settings.borrow().search.maximum_results as f64);
    let weak = Rc::downgrade(launcher);
    limit.connect_value_notify(move |row| {
        if let Some(launcher) = weak.upgrade() {
            launcher.settings().borrow_mut().search.maximum_results = row.value() as usize;
            persist(&launcher, false);
        }
    });
    group.add(&limit);
    let latency = adw::SwitchRow::builder()
        .title("Show Search Timing")
        .subtitle("Optional worker latency in the footer; UI timing is in Diagnostics")
        .active(settings.borrow().search.show_latency)
        .build();
    let weak = Rc::downgrade(launcher);
    latency.connect_active_notify(move |row| {
        if let Some(launcher) = weak.upgrade() {
            launcher.settings().borrow_mut().search.show_latency = row.is_active();
            persist(&launcher, false);
        }
    });
    group.add(&latency);
    page.add(&group);
    page
}

fn keyboard_page(launcher: &Rc<Launcher>, dialog: &adw::PreferencesDialog) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Keyboard")
        .icon_name("preferences-desktop-keyboard-shortcuts-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder()
        .title("Global Shortcut")
        .description("Managed by your desktop through the Wayland-safe XDG portal")
        .build();
    let shortcut = adw::ActionRow::builder()
        .title("Open Launcher")
        .use_markup(false)
        .build();
    launcher.refresh_shortcut_notice();
    shortcut.set_subtitle(
        &launcher
            .shortcut_notice
            .state()
            .and_then(|v| v.get::<String>())
            .unwrap_or_default(),
    );
    let row = shortcut.downgrade();
    let handler = launcher
        .shortcut_notice
        .connect_state_notify(move |action| {
            if let Some(row) = row.upgrade() {
                row.set_subtitle(
                    &action
                        .state()
                        .and_then(|v| v.get::<String>())
                        .unwrap_or_default(),
                );
            }
        });
    let action = launcher.shortcut_notice.clone();
    let handler = RefCell::new(Some(handler));
    dialog.connect_closed(move |_| {
        if let Some(handler) = handler.borrow_mut().take() {
            action.disconnect(handler);
        }
    });
    group.add(&shortcut);
    let controls = adw::ActionRow::builder().title("Choose a Shortcut").subtitle("Save your preference, then confirm the combination in your desktop’s dialog. The desktop owns the final binding.").build();
    let record = gtk::Button::with_label("Record Shortcut");
    record.set_valign(gtk::Align::Center);
    let weak = Rc::downgrade(launcher);
    let parent = dialog.downgrade();
    record.connect_clicked(move |_| {
        if let (Some(launcher), Some(parent)) = (weak.upgrade(), parent.upgrade()) {
            record_shortcut(&launcher, &parent);
        }
    });
    controls.add_suffix(&record);
    let reset = gtk::Button::with_label("Reset to Alt+Space");
    reset.set_valign(gtk::Align::Center);
    let weak = Rc::downgrade(launcher);
    reset.connect_clicked(move |_| {
        if let Some(launcher) = weak.upgrade() {
            save_shortcut(&launcher, DEFAULT_LAUNCHER_SHORTCUT.to_owned());
        }
    });
    controls.add_suffix(&reset);
    group.add(&controls);
    let configure_row = adw::ActionRow::builder().title("Desktop Confirmation").subtitle("Retry authorization or open the desktop’s shortcut settings. No existing GNOME keybindings are changed by Spotlight.").build();
    let configure = gtk::Button::with_label("Configure / Retry");
    configure.set_valign(gtk::Align::Center);
    configure.add_css_class("flat");
    let weak = Rc::downgrade(launcher);
    configure.connect_clicked(move |_| {
        if let Some(launcher) = weak.upgrade() {
            launcher.shortcut_service().configure(
                launcher
                    .settings()
                    .borrow()
                    .keyboard
                    .launcher_shortcut
                    .clone(),
            );
        }
    });
    configure_row.add_suffix(&configure);
    group.add(&configure_row);
    page.add(&group);
    page
}

fn record_shortcut(launcher: &Rc<Launcher>, parent: &adw::PreferencesDialog) {
    let dialog = adw::AlertDialog::builder().heading("Record Shortcut")
        .body("Press a combination containing Alt, Ctrl, or Super. Escape cancels. Saving opens the desktop’s confirmation; it never overrides a GNOME binding.")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("save", "Save & Configure");
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("save", false);
    dialog.set_close_response("cancel");
    let preview = gtk::Label::builder()
        .label("Waiting for a key combination…")
        .wrap(true)
        .build();
    preview.set_margin_top(12);
    preview.set_margin_bottom(12);
    dialog.set_extra_child(Some(&preview));
    let pending = Rc::new(RefCell::new(None::<String>));
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let dialog_weak = dialog.downgrade();
    let value = Rc::clone(&pending);
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        if key == gtk::gdk::Key::Escape {
            return gtk::glib::Propagation::Proceed;
        }
        let Some(shortcut) = shortcut_keys::record(key, modifiers) else {
            return gtk::glib::Propagation::Proceed;
        };
        let mut text = shortcut_keys::label(&shortcut);
        if let Some(conflict) = shortcut_keys::gnome_conflict(&shortcut) {
            text.push_str(&format!("\n\n{conflict}"));
        }
        preview.set_label(&text);
        *value.borrow_mut() = Some(shortcut);
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.set_response_enabled("save", true);
        }
        gtk::glib::Propagation::Stop
    });
    dialog.add_controller(controller);
    let weak = Rc::downgrade(launcher);
    dialog.connect_response(None, move |_, response| {
        if response == "save"
            && let (Some(launcher), Some(shortcut)) = (weak.upgrade(), pending.borrow().clone())
        {
            save_shortcut(&launcher, shortcut);
        }
    });
    dialog.present(Some(parent));
}

fn save_shortcut(launcher: &Rc<Launcher>, shortcut: String) {
    let settings = launcher.settings();
    let previous = settings.borrow().keyboard.launcher_shortcut.clone();
    settings.borrow_mut().keyboard.launcher_shortcut = shortcut.clone();
    let saved = launcher.settings_store().save(&settings.borrow());
    if let Err(error) = saved {
        settings.borrow_mut().keyboard.launcher_shortcut = previous;
        launcher.show_toast(&format!("Could not save shortcut: {error}"));
        return;
    }
    launcher.refresh_shortcut_notice();
    launcher.shortcut_service().configure(shortcut);
}

fn privacy_page(launcher: &Rc<Launcher>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Privacy")
        .icon_name("channel-secure-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder()
        .title("Local Usage Ranking")
        .description("Stored only on this computer; no telemetry or network requests")
        .build();
    let enabled = adw::SwitchRow::builder()
        .title("Learn From Opened Results")
        .subtitle("Frequently used applications gradually rank higher")
        .active(launcher.settings().borrow().privacy.usage_history)
        .build();
    let weak = Rc::downgrade(launcher);
    enabled.connect_active_notify(move |row| {
        let Some(launcher) = weak.upgrade() else {
            return;
        };
        launcher.settings().borrow_mut().privacy.usage_history = row.is_active();
        persist(&launcher, false);
    });
    group.add(&enabled);

    let clear = adw::ActionRow::builder()
        .title("Clear Usage History")
        .subtitle("Deletes launch counts and recency data")
        .build();
    let button = gtk::Button::with_label("Clear");
    button.add_css_class("destructive-action");
    button.set_valign(gtk::Align::Center);
    let weak = Rc::downgrade(launcher);
    button.connect_clicked(move |_| {
        if let Some(launcher) = weak.upgrade() {
            launcher.clear_history();
        }
    });
    clear.add_suffix(&button);
    group.add(&clear);
    page.add(&group);
    page
}

fn advanced_page(launcher: &Rc<Launcher>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Advanced")
        .icon_name("applications-engineering-symbolic")
        .build();
    let rendering = adw::PreferencesGroup::builder().title("Rendering").description("Takes effect after Quit Spotlight and reopening. OpenGL reduces idle driver activity on the tested NVIDIA system; memory costs vary. An explicit GSK_RENDERER environment override takes precedence.").build();
    use spotlight_core::settings::RendererPreference;
    rendering.add(&choice(
        launcher,
        "Renderer",
        &["OpenGL", "GTK Default", "Vulkan", "Software (Cairo)"],
        match launcher.settings().borrow().general.renderer {
            RendererPreference::OpenGl => 0,
            RendererPreference::GtkDefault => 1,
            RendererPreference::Vulkan => 2,
            RendererPreference::Software => 3,
        },
        false,
        |s, i| {
            s.general.renderer = match i {
                1 => RendererPreference::GtkDefault,
                2 => RendererPreference::Vulkan,
                3 => RendererPreference::Software,
                _ => RendererPreference::OpenGl,
            };
        },
    ));
    rendering.add(&detail_row("Active Renderer", &launcher.renderer_name()));
    page.add(&rendering);
    let group = adw::PreferencesGroup::builder()
        .title("Diagnostics")
        .description("Diagnostics contain counts and timings, never search text")
        .build();
    let diagnostics = launcher.diagnostics();
    group.add(&detail_row(
        "Desktop Session",
        &format!(
            "{} on {}",
            std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "Unknown desktop".into()),
            std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown display server".into())
        ),
    ));
    group.add(&detail_row(
        "Application Index",
        &format!(
            "{} indexed from {} files; {} invalid entries skipped",
            diagnostics.application_count, diagnostics.files_seen, diagnostics.invalid_entries
        ),
    ));
    let portal = diagnostics.shortcut.portal_version.map_or_else(
        || "Unavailable or still connecting".into(),
        |version| format!("XDG Global Shortcuts portal v{version}"),
    );
    group.add(&detail_row("Shortcut Backend", &portal));
    group.add(&detail_row("Result List Updates", &format!(
        "Last {} µs · maximum {} µs · {} reusable rows. Widget-update time, not compositor latency.",
        diagnostics.ui_update_micros, diagnostics.maximum_ui_update_micros, diagnostics.allocated_rows
    )));
    if let Some(latency) = diagnostics.latency {
        group.add(&detail_row(
            "Application Search Latency",
            &format!(
                "{} samples — p50 {} µs, p95 {} µs, max {} µs",
                latency.sample_count,
                latency.p50_micros,
                latency.p95_micros,
                latency.maximum_micros
            ),
        ));
    }
    page.add(&group);
    page
}

fn about_page() -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("About")
        .icon_name("help-about-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder()
        .title("Spotlight Linux")
        .description("A fast, local-first command surface for Linux")
        .build();
    group.add(&detail_row("Version", env!("CARGO_PKG_VERSION")));
    group.add(&detail_row("Telemetry", "None"));
    group.add(&detail_row(
        "Network",
        "No Phase 1 feature performs network requests",
    ));
    page.add(&group);
    page
}

fn detail_row(title: &str, subtitle: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build()
}

fn persist(launcher: &Rc<Launcher>, appearance_changed: bool) {
    let settings = launcher.settings();
    if let Err(error) = launcher.settings_store().save(&settings.borrow()) {
        launcher.show_toast(&format!("Could not save Settings: {error}"));
        return;
    }
    if appearance_changed {
        launcher.apply_settings();
    } else {
        launcher.apply_search_settings();
    }
}
