//! Per-application GTK renderer preference. Never change the desktop environment.
use std::{ffi::OsStr, os::unix::process::CommandExt, process::Command};

use gtk::{gdk, gio, prelude::*};
use spotlight_core::settings::RendererPreference;

const INTERNAL: &str = "SPOTLIGHT_INTERNAL_RENDERER";

/// Re-exec once with an explicit environment, before GTK initializes. This
/// avoids unsafe process-wide set_var after GLib or driver threads may exist.
/// Explicit user GSK_RENDERER overrides (including test renderers) always win.
pub fn prepare(preference: RendererPreference) -> std::io::Result<()> {
    let Some(renderer) = preference.environment_value() else {
        return Ok(());
    };
    if std::env::var_os("GSK_RENDERER").is_some() {
        return Ok(());
    }
    Err(Command::new(std::env::current_exe()?)
        .args(std::env::args_os().skip(1))
        .env("GSK_RENDERER", renderer)
        .env(INTERNAL, renderer)
        .exec())
}

fn internal_override(renderer: Option<&OsStr>, marker: Option<&OsStr>) -> bool {
    renderer == marker
        && marker.is_some_and(|s| ["gl", "vulkan", "cairo"].iter().any(|value| s == *value))
}

fn is_internal() -> bool {
    internal_override(
        std::env::var_os("GSK_RENDERER").as_deref(),
        std::env::var_os(INTERNAL).as_deref(),
    )
}

/// Do not pass Spotlight's preference into applications launched by a result.
pub fn launch_context() -> gio::AppLaunchContext {
    let context = gdk::Display::default()
        .map(|display| display.app_launch_context().upcast())
        .unwrap_or_default();
    clean_context(&context, is_internal());
    context
}

fn clean_context(context: &gio::AppLaunchContext, internal: bool) {
    if internal {
        context.unsetenv("GSK_RENDERER");
    }
    context.unsetenv(INTERNAL);
}

pub fn subprocess_launcher() -> gio::SubprocessLauncher {
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::NONE);
    if is_internal() {
        launcher.unsetenv("GSK_RENDERER");
    }
    launcher.unsetenv(INTERNAL);
    launcher
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_override_is_local_and_user_environment_is_preserved() {
        assert!(!internal_override(None, None));
        assert!(!internal_override(
            Some("vulkan".as_ref()),
            Some("gl".as_ref())
        ));
        assert!(internal_override(Some("gl".as_ref()), Some("gl".as_ref())));
        let context = gio::AppLaunchContext::new();
        context.setenv("GSK_RENDERER", "gl");
        context.setenv(INTERNAL, "gl");
        clean_context(&context, true);
        assert!(
            !context
                .environment()
                .iter()
                .any(|value| value.as_encoded_bytes().starts_with(b"GSK_RENDERER="))
        );
        context.setenv("GSK_RENDERER", "vulkan");
        clean_context(&context, false);
        assert!(
            context
                .environment()
                .iter()
                .any(|value| value == "GSK_RENDERER=vulkan")
        );
    }
}
