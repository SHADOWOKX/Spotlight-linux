mod actions;
mod activation_trace;
mod backend;
mod launcher;
mod platform;
mod preferences;
mod result_row;
mod style;

use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;
use gtk::{gio, glib};
use spotlight_core::settings::{APPLICATION_ID, Settings, SettingsStore, XdgPaths};
use tracing_subscriber::EnvFilter;

use crate::launcher::Launcher;

fn main() -> glib::ExitCode {
    initialize_logging();
    if let Some(operation) =
        std::env::args().find(|arg| matches!(arg.as_str(), "--install-user" | "--uninstall-user"))
    {
        return maintenance(&operation);
    }
    let paths = match XdgPaths::from_process() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("Spotlight Linux cannot determine its data directories: {error}");
            return glib::ExitCode::FAILURE;
        }
    };
    let settings_store = SettingsStore::new(paths.settings_file());
    let (settings, settings_warning) = match settings_store.load() {
        Ok(settings) => (settings, None),
        Err(error) => {
            tracing::warn!(%error, "settings could not be loaded; using safe defaults");
            (
                Settings::default(),
                Some(format!(
                    "Settings could not be loaded; safe defaults are active: {error}"
                )),
            )
        }
    };

    if let Err(error) = platform::rendering::prepare(settings.general.renderer) {
        eprintln!("Could not initialize Spotlight's renderer preference: {error}");
        return glib::ExitCode::FAILURE;
    }

    let application = adw::Application::builder()
        .application_id(APPLICATION_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    for (option, description) in [
        (
            "background",
            "Keep the shortcut ready without showing a window",
        ),
        ("toggle", "Toggle the existing launcher"),
        ("hide", "Dismiss the launcher without quitting"),
        ("settings", "Open Spotlight Linux Settings"),
        ("quit", "Quit the resident launcher"),
        (
            "diagnostics",
            "Print local runtime diagnostics (no queries or clipboard data)",
        ),
    ] {
        application.add_main_option(
            option,
            glib::Char::from(0),
            glib::OptionFlags::NONE,
            glib::OptionArg::None,
            description,
            None,
        );
    }
    let launcher = Rc::new(RefCell::<Option<Rc<Launcher>>>::new(None));
    let resident = Rc::new(RefCell::new(None::<gio::ApplicationHoldGuard>));
    let launcher_slot = Rc::clone(&launcher);
    let hold = Rc::clone(&resident);
    application.connect_startup(move |application| {
        // GlobalShortcuts sessions are process-owned. A single explicit hold
        // keeps the warmed UI/index available when every window is hidden.
        *hold.borrow_mut() = Some(application.hold());
        let launcher = Launcher::new(
            application,
            settings.clone(),
            settings_store.clone(),
            paths.clone(),
            true,
        );
        if let Some(message) = settings_warning.as_deref() {
            launcher.show_toast(message);
        }
        *launcher_slot.borrow_mut() = Some(launcher);
    });

    let slot = Rc::clone(&launcher);
    application.connect_activate(move |_| {
        if let Some(launcher) = slot.borrow().as_ref() {
            launcher.present();
        }
    });
    for name in ["show", "toggle", "hide", "settings", "quit"] {
        let action = gio::SimpleAction::new(name, None);
        let slot = Rc::clone(&launcher);
        let app = application.downgrade();
        action.connect_activate(move |_, _| {
            if name == "quit" {
                if let Some(app) = app.upgrade() {
                    app.quit();
                }
                return;
            }
            if let Some(launcher) = slot.borrow().as_ref() {
                dispatch(launcher, name);
            }
        });
        application.add_action(&action);
    }
    let slot = Rc::clone(&launcher);
    application.connect_command_line(move |application, command| {
        let options = command.options_dict();
        if options.contains("quit") {
            application.quit();
            return glib::ExitCode::SUCCESS;
        }
        let slot = slot.borrow();
        let Some(launcher) = slot.as_ref() else {
            return glib::ExitCode::FAILURE;
        };
        if options.contains("diagnostics") {
            command.print_literal(&launcher.runtime_diagnostics());
        } else if options.contains("background") { /* Do not hide an already-visible instance. */
        } else if options.contains("hide") {
            dispatch(launcher, "hide");
        } else if options.contains("settings") {
            dispatch(launcher, "settings");
        } else if options.contains("toggle") {
            dispatch(launcher, "toggle");
        } else {
            dispatch(launcher, "show");
        }
        glib::ExitCode::SUCCESS
    });

    let exit = application.run();
    launcher.borrow_mut().take();
    resident.borrow_mut().take();
    exit
}

fn dispatch(launcher: &Rc<Launcher>, action: &str) {
    match action {
        "hide" => launcher.hide(),
        "toggle" => launcher.toggle(),
        "settings" => {
            launcher.present();
            launcher.present_preferences();
        }
        _ => launcher.present(),
    }
}

fn maintenance(operation: &str) -> glib::ExitCode {
    use platform::installation::{self, InstallPaths};
    let result = (|| -> std::io::Result<_> {
        let paths = InstallPaths::from_process()?;
        installation::stop_running(&paths)?;
        if operation == "--install-user" {
            installation::install(&paths, &std::env::current_exe()?)
        } else {
            installation::uninstall(&paths)
        }
    })();
    match result {
        Ok(paths) => {
            for path in paths {
                println!(
                    "{}: {}",
                    if operation == "--install-user" {
                        "Installed"
                    } else {
                        "Removed"
                    },
                    path.display()
                );
            }
            glib::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Spotlight Linux installation operation failed: {error}");
            glib::ExitCode::FAILURE
        }
    }
}

fn initialize_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("spotlight_linux=info,spotlight_core=warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .compact()
        .try_init();
}
