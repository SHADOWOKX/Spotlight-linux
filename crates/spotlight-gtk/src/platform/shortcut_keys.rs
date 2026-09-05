//! Recorder syntax and read-only desktop conflict diagnostics.
use gtk::{gdk, gio, prelude::*};
use spotlight_core::settings::validate_shortcut;

pub fn record(key: gdk::Key, modifiers: gdk::ModifierType) -> Option<String> {
    if matches!(
        key,
        gdk::Key::Shift_L
            | gdk::Key::Shift_R
            | gdk::Key::Control_L
            | gdk::Key::Control_R
            | gdk::Key::Alt_L
            | gdk::Key::Alt_R
            | gdk::Key::Super_L
            | gdk::Key::Super_R
            | gdk::Key::Meta_L
            | gdk::Key::Meta_R
            | gdk::Key::Caps_Lock
    ) {
        return None;
    }
    if !modifiers.intersects(
        gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SUPER_MASK,
    ) {
        return None;
    }
    let mut parts = Vec::new();
    for (mask, name) in [
        (gdk::ModifierType::CONTROL_MASK, "CTRL"),
        (gdk::ModifierType::ALT_MASK, "ALT"),
        (gdk::ModifierType::SHIFT_MASK, "SHIFT"),
        (gdk::ModifierType::SUPER_MASK, "LOGO"),
    ] {
        if modifiers.contains(mask) {
            parts.push(name.to_owned());
        }
    }
    parts.push(key.to_lower().name()?.to_string());
    let shortcut = parts.join("+");
    validate_shortcut(&shortcut).ok().map(|()| shortcut)
}

fn accelerator(trigger: &str) -> String {
    trigger
        .split('+')
        .map(|part| match part {
            "CTRL" => "<Control>",
            "ALT" => "<Alt>",
            "SHIFT" => "<Shift>",
            "LOGO" => "<Super>",
            "NUM" => "<Mod2>",
            other => other,
        })
        .collect()
}

pub fn label(trigger: &str) -> String {
    gtk::accelerator_parse(accelerator(trigger))
        .map(|(key, mods)| gtk::accelerator_get_label(key, mods).to_string())
        .unwrap_or_else(|| trigger.to_owned())
}

pub fn gnome_conflict(trigger: &str) -> Option<String> {
    let requested = gtk::accelerator_parse(accelerator(trigger))?;
    let source = gio::SettingsSchemaSource::default()?;
    for schema_id in [
        "org.gnome.desktop.wm.keybindings",
        "org.gnome.mutter.keybindings",
        "org.gnome.shell.keybindings",
        "org.gnome.settings-daemon.plugins.media-keys",
    ] {
        let Some(schema) = source.lookup(schema_id, true) else {
            continue;
        };
        let settings = gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None);
        for name in schema.list_keys() {
            let Some(bindings) = settings.value(&name).get::<Vec<String>>() else {
                continue;
            };
            if bindings
                .iter()
                .any(|binding| gtk::accelerator_parse(binding) == Some(requested))
            {
                let action = if name == "activate-window-menu" {
                    "Window Menu".to_owned()
                } else {
                    name.replace('-', " ")
                };
                return Some(format!(
                    "{} is already assigned to GNOME’s {action}. Choose another shortcut, or change that binding yourself in GNOME Settings. Spotlight has not changed it.",
                    label(trigger)
                ));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_portal_syntax_and_ignores_modifier_only_keys() {
        assert_eq!(
            record(gdk::Key::space, gdk::ModifierType::ALT_MASK).as_deref(),
            Some("ALT+space")
        );
        assert_eq!(
            record(
                gdk::Key::K,
                gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK
            )
            .as_deref(),
            Some("CTRL+SHIFT+k")
        );
        assert_eq!(record(gdk::Key::a, gdk::ModifierType::empty()), None);
        assert_eq!(record(gdk::Key::Alt_L, gdk::ModifierType::ALT_MASK), None);
    }
}
