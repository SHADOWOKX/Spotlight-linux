use adw::prelude::*;
use gtk::gdk;
use spotlight_core::settings::{
    Accent, AnimationPreference, CornerRadius, Density, Palette, Settings, Theme, WindowStyle,
};

const BASE_CSS: &str = include_str!("../../../data/style.css");

pub fn install() -> gtk::CssProvider {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(BASE_CSS);
    let dynamic = gtk::CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        gtk::style_context_add_provider_for_display(
            &display,
            &dynamic,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }
    dynamic
}

pub fn apply(window: &adw::ApplicationWindow, dynamic: &gtk::CssProvider, settings: &Settings) {
    let manager = adw::StyleManager::default();
    manager.set_color_scheme(match settings.appearance.theme {
        Theme::System => adw::ColorScheme::Default,
        Theme::Light => adw::ColorScheme::ForceLight,
        Theme::Dark => adw::ColorScheme::ForceDark,
    });
    for class in [
        "style-normal",
        "style-glass",
        "style-minimal",
        "radius-small",
        "radius-medium",
        "radius-large",
        "motion-reduced",
        "motion-off",
        "density-compact",
    ] {
        window.remove_css_class(class);
    }

    window.add_css_class(match settings.appearance.window_style {
        WindowStyle::Normal => "style-normal",
        WindowStyle::Glass => "style-glass",
        WindowStyle::Minimal => "style-minimal",
    });
    window.add_css_class(match settings.appearance.corner_radius {
        CornerRadius::Small => "radius-small",
        CornerRadius::Medium => "radius-medium",
        CornerRadius::Large => "radius-large",
    });
    let motion = if gtk::Settings::default().is_some_and(|s| !s.is_gtk_enable_animations()) {
        AnimationPreference::Off
    } else {
        settings.appearance.animations
    };
    match motion {
        AnimationPreference::Full => {}
        AnimationPreference::Reduced => window.add_css_class("motion-reduced"),
        AnimationPreference::Off => window.add_css_class("motion-off"),
    }
    if settings.appearance.density == Density::Compact {
        window.add_css_class("density-compact");
    }

    let opacity = if settings.appearance.window_style == WindowStyle::Glass {
        settings.appearance.transparency
    } else {
        1.0
    };
    let accent = match settings.appearance.accent {
        Accent::Graphite => "@window_fg_color",
        Accent::System => "@accent_bg_color",
        Accent::Blue => "#3584e4",
        Accent::Violet => "#9141ac",
        Accent::Green => "#26a269",
    };
    let strength = if settings.appearance.accent == Accent::Graphite {
        0.09
    } else {
        0.18
    };
    let height = settings.appearance.result_row_height.pixels();
    let (background, foreground) = match (settings.appearance.palette, manager.is_dark()) {
        (Palette::Native, _) => ("@window_bg_color", "@window_fg_color"),
        (Palette::Graphite, true) => ("#202124", "#f1f1f3"),
        (Palette::Midnight, true) => ("#171e2b", "#eaf0fb"),
        (Palette::Dusk, true) => ("#26202e", "#f5edf9"),
        (Palette::Forest, true) => ("#1b2723", "#edf5f0"),
        (Palette::Graphite, false) => ("#f4f4f5", "#222329"),
        (Palette::Midnight, false) => ("#eef2f9", "#1c293e"),
        (Palette::Dusk, false) => ("#f6f0f8", "#302139"),
        (Palette::Forest, false) => ("#eff5f0", "#20382b"),
    };
    let accent = if settings.appearance.accent == Accent::Graphite {
        foreground
    } else {
        accent
    };
    let search_font = settings.appearance.search_font_size;
    let result_font = settings.appearance.result_font_size;
    dynamic.load_from_string(&format!(
        ".launcher-window .launcher-surface {{ background-color: alpha({background}, {opacity:.3}); color: {foreground}; }}
         .launcher-window .result-row {{ color: {foreground}; }}
         .launcher-window .launcher-search {{ font-size: {search_font}px; color: {foreground}; }}
         .launcher-window .result-title {{ font-size: {result_font}px; }}
         .launcher-window .section-heading, .launcher-window .launcher-footer, .launcher-window .footer-button, .launcher-window .search-scope, .launcher-window .empty-state {{ color: alpha({foreground}, 0.65); }}
         .launcher-window .result-row {{ min-height: {height}px; }}
         .launcher-window .result-row:selected {{ background-color: alpha({accent}, {strength}); color: {foreground}; }}"
    ));
    window.set_default_width(settings.appearance.window_width.pixels());
    let manager = adw::StyleManager::default();
    manager.set_color_scheme(match settings.appearance.theme {
        Theme::System => adw::ColorScheme::Default,
        Theme::Light => adw::ColorScheme::ForceLight,
        Theme::Dark => adw::ColorScheme::ForceDark,
    });
}
