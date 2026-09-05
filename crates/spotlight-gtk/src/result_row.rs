use std::cell::RefCell;

use adw::prelude::*;
use spotlight_core::{Icon, SearchResult, settings::AppearanceSettings};

/// Reusable widget slot. Typing updates labels instead of rebuilding the tree.
pub struct ResultRow {
    pub widget: gtk::ListBoxRow,
    icon: gtk::Image,
    title: gtk::Label,
    subtitle: gtk::Label,
    kind: gtk::Label,
    last_icon: RefCell<Option<Icon>>,
}

impl ResultRow {
    pub fn new() -> Self {
        let widget = gtk::ListBoxRow::new();
        widget.add_css_class("result-row");
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let icon = gtk::Image::new();
        icon.add_css_class("result-icon");
        content.append(&icon);
        let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
        labels.set_hexpand(true);
        labels.set_valign(gtk::Align::Center);
        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        title.add_css_class("result-title");
        let subtitle = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        subtitle.add_css_class("result-subtitle");
        labels.append(&title);
        labels.append(&subtitle);
        content.append(&labels);
        let kind = gtk::Label::new(Some("Application"));
        kind.add_css_class("result-kind");
        content.append(&kind);
        let hint = gtk::Label::new(Some("↵"));
        hint.add_css_class("shortcut-hint");
        hint.set_valign(gtk::Align::Center);
        content.append(&hint);
        widget.set_child(Some(&content));
        Self {
            widget,
            icon,
            title,
            subtitle,
            kind,
            last_icon: RefCell::new(None),
        }
    }

    pub fn update(&self, result: &SearchResult, appearance: &AppearanceSettings) {
        self.kind.set_label(if result.provider.0 == "calculator" {
            "Calculator"
        } else {
            "Application"
        });
        self.kind.set_visible(appearance.show_result_type);
        if self.last_icon.borrow().as_ref() != Some(&result.icon) {
            match &result.icon {
                Icon::Themed(name) => self.icon.set_icon_name(Some(name)),
                Icon::File(path) => self.icon.set_from_file(Some(path)),
                Icon::Text(_) => self
                    .icon
                    .set_icon_name(Some("application-x-executable-symbolic")),
            }
            *self.last_icon.borrow_mut() = Some(result.icon.clone());
        }
        self.icon.set_pixel_size(appearance.icon_size.pixels());
        self.title.set_label(&result.title);
        let subtitle = result
            .subtitle
            .as_deref()
            .filter(|s| *s != "Application")
            .unwrap_or("");
        self.subtitle.set_label(subtitle);
        self.subtitle
            .set_visible(appearance.show_subtitles && !subtitle.is_empty());
        self.widget.set_tooltip_text(Some(&result.title));
    }
}
