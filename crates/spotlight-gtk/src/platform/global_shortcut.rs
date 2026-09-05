//! Event-driven, connection-scoped portal integration. No keyboard hooks or polling.
use std::{cell::Cell, rc::Rc};

use ashpd::{
    AppID,
    desktop::{
        CreateSessionOptions,
        global_shortcuts::{
            BindShortcutsOptions, ConfigureShortcutsOptions, GlobalShortcuts, NewShortcut, Shortcut,
        },
    },
};
use async_channel::{Receiver, Sender};
use futures_util::{FutureExt, StreamExt};
use gtk::gio;
use spotlight_core::settings::APPLICATION_ID;

use crate::activation_trace::{ActivationEvent, ActivationTrace};

const TOGGLE_SHORTCUT_ID: &str = "toggle-launcher";

#[derive(Clone, Debug)]
pub enum ShortcutEvent {
    Connecting,
    Ready {
        portal_version: u32,
        trigger_description: Option<String>,
        connection: String,
        session: String,
        awaiting_approval: bool,
    },
    Changed {
        trigger_description: Option<String>,
    },
    Activated {
        activation_token: Option<String>,
    },
    Notice {
        message: String,
    },
    Failed {
        message: String,
    },
}

struct Watch(Option<Box<dyn FnOnce()>>);
impl Drop for Watch {
    fn drop(&mut self) {
        if let Some(unwatch) = self.0.take() {
            unwatch();
        }
    }
}

#[derive(Clone)]
pub struct ShortcutService {
    commands: Sender<ShortcutCommand>,
    _watch: Rc<Watch>,
    metrics: Rc<Cell<ShortcutMetrics>>,
    pub trace: ActivationTrace,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ShortcutMetrics {
    pub activations: u64,
    pub deactivations: u64,
    pub forwarded: u64,
    pub last_has_token: bool,
}

#[derive(Clone, Debug)]
enum ShortcutCommand {
    Configure(String),
    Reconnect,
    Unavailable,
}

impl ShortcutService {
    pub fn start(mut preferred: String) -> (Self, Receiver<ShortcutEvent>) {
        let (commands, receiver) = async_channel::unbounded();
        let (events, event_receiver) = async_channel::unbounded();
        let metrics = Rc::new(Cell::new(ShortcutMetrics::default()));
        let observed = Rc::clone(&metrics);
        let trace = ActivationTrace::default();
        let observed_trace = trace.clone();
        let appeared = commands.clone();
        let vanished = commands.clone();
        let watch = gio::bus_watch_name(
            gio::BusType::Session,
            "org.freedesktop.portal.Desktop",
            gio::BusNameWatcherFlags::AUTO_START,
            move |_, _, _| {
                let _ = appeared.try_send(ShortcutCommand::Reconnect);
            },
            move |_, _| {
                let _ = vanished.try_send(ShortcutCommand::Unavailable);
            },
        );
        gtk::glib::MainContext::default().spawn_local(async move {
            let mut pending = None;
            // Errors return here, not out of the task: Record / Configure can retry.
            loop {
                let command = match pending.take() { Some(command) => command, None => match receiver.recv().await { Ok(command) => command, Err(_) => break } };
                match command {
                    ShortcutCommand::Unavailable => {
                        let _ = events.try_send(ShortcutEvent::Failed { message: "The desktop portal is unavailable. Local search still works. Retry from Keyboard settings when the desktop portal is running.".into() });
                        continue;
                    }
                    ShortcutCommand::Configure(value) => preferred = value,
                    ShortcutCommand::Reconnect => (),
                }
                let _ = events.try_send(ShortcutEvent::Connecting);
                match run_portal(&mut preferred, &receiver, &events, &observed, &observed_trace).await {
                    Ok(true) => pending = Some(ShortcutCommand::Reconnect),
                    Ok(false) => break,
                    Err(message) => { let _ = events.try_send(ShortcutEvent::Failed { message }); }
                }
            }
        });
        let unwatch: Box<dyn FnOnce()> = Box::new(move || gio::bus_unwatch_name(watch));
        (
            Self {
                commands,
                _watch: Rc::new(Watch(Some(unwatch))),
                metrics,
                trace,
            },
            event_receiver,
        )
    }

    pub fn configure(&self, preferred: String) {
        let _ = self
            .commands
            .try_send(ShortcutCommand::Configure(preferred));
    }

    pub fn metrics(&self) -> ShortcutMetrics {
        self.metrics.get()
    }
}

async fn run_portal(
    preferred: &mut String,
    commands: &Receiver<ShortcutCommand>,
    events: &Sender<ShortcutEvent>,
    metrics: &Cell<ShortcutMetrics>,
    trace: &ActivationTrace,
) -> Result<bool, String> {
    let connection = zbus::Connection::session()
        .await
        .map_err(|e| e.to_string())?;
    let connection_name = connection
        .unique_name()
        .map(ToString::to_string)
        .unwrap_or_default();
    // Registry identity belongs to THIS connection. Register first, before even
    // reading portal properties, and repeat on a fresh connection after restart.
    if !ashpd::is_sandboxed() {
        ashpd::register_host_app_with_connection(connection.clone(), AppID::try_from(APPLICATION_ID).map_err(|e| e.to_string())?)
            .await.map_err(|error| format!("Desktop identity registration failed: {error}. Reinstall Spotlight Linux with scripts/install-user.sh, then reopen it from GNOME. The installed desktop entry must resolve to an absolute executable path."))?;
    }
    let portal = GlobalShortcuts::with_connection(connection)
        .await
        .map_err(humanize)?;
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await
        .map_err(humanize)?;
    // Session implements the D-Bus object-path wire type but keeps its accessor
    // private. Decode that public representation once, never parse Debug output.
    let bytes = zbus::zvariant::to_bytes(
        zbus::zvariant::serialized::Context::new_dbus(zbus::zvariant::LE, 0),
        &session,
    )
    .map_err(|e| e.to_string())?;
    let (session_path, _) = bytes
        .deserialize::<zbus::zvariant::OwnedObjectPath>()
        .map_err(|e| e.to_string())?;
    let mut activations = portal.receive_activated().await.map_err(humanize)?;
    let mut deactivations = portal.receive_deactivated().await.map_err(humanize)?;
    let mut changes = portal.receive_shortcuts_changed().await.map_err(humanize)?;
    let mut closed = session.receive_closed().await.map_err(humanize)?;
    let _ = events.try_send(ShortcutEvent::Ready {
        portal_version: portal.version(),
        trigger_description: None,
        connection: connection_name.clone(),
        session: session_path.to_string(),
        awaiting_approval: true,
    });
    let shortcut = NewShortcut::new(TOGGLE_SHORTCUT_ID, "Open Spotlight Linux")
        .preferred_trigger(Some(preferred.as_str()));
    let response = portal
        .bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
        .await
        .map_err(humanize)?
        .response()
        .map_err(humanize)?;
    let _ = events.try_send(ShortcutEvent::Ready {
        portal_version: portal.version(),
        trigger_description: trigger_description(response.shortcuts()),
        connection: connection_name,
        session: session_path.to_string(),
        awaiting_approval: false,
    });
    loop {
        let activation = activations.next().fuse();
        let deactivation = deactivations.next().fuse();
        let change = changes.next().fuse();
        let closure = closed.next().fuse();
        let command = commands.recv().fuse();
        futures_util::pin_mut!(activation, deactivation, change, closure, command);
        futures_util::select! {
            activation = activation => {
                let Some(activation) = activation else { return Err("Shortcut activation stream closed. Retry from Keyboard settings.".into()) };
                if is_our_shortcut(activation.session_handle().as_str(), session_path.as_str(), activation.shortcut_id()) {
                    let activation_token = activation.options().get("activation_token").and_then(|v| v.downcast_ref::<&str>().ok()).filter(|token| !token.is_empty()).map(str::to_owned);
                    let mut value = metrics.get();
                    value.activations += 1;
                    value.last_has_token = activation_token.is_some();
                    trace.record(ActivationEvent::Activated { timestamp: activation.timestamp().as_millis(), has_token: value.last_has_token });
                    // A release is not a prerequisite for another legitimate
                    // activation. Some desktops do not deliver paired releases
                    // when focus changes. Each Activated toggles once in the UI;
                    // Deactivated remains diagnostic only.
                    if events.try_send(ShortcutEvent::Activated { activation_token }).is_ok() {
                        value.forwarded += 1;
                    }
                    metrics.set(value);
                }
            },
            deactivation = deactivation => {
                let Some(deactivation) = deactivation else { return Err("Shortcut release stream closed. Retry from Keyboard settings.".into()) };
                if is_our_shortcut(deactivation.session_handle().as_str(), session_path.as_str(), deactivation.shortcut_id()) {
                    let mut value = metrics.get(); value.deactivations += 1; metrics.set(value);
                    trace.record(ActivationEvent::Deactivated { timestamp: deactivation.timestamp().as_millis() });
                }
                // Diagnostic only: never toggle, focus, hide, or gate Activated.
            },
            change = change => {
                let Some(change) = change else { return Err("Shortcut configuration stream closed. Retry from Keyboard settings.".into()) };
                if change.session_handle().as_str() == session_path.as_str() {
                    let _ = events.try_send(ShortcutEvent::Changed { trigger_description: trigger_description(change.shortcuts()) });
                }
            },
            _ = closure => return Err("The desktop closed the shortcut session. Open Keyboard settings and retry authorization.".into()),
            command = command => match command {
                Ok(ShortcutCommand::Configure(value)) => {
                    *preferred = value;
                    let result = if portal.version() >= 2 {
                        portal.configure_shortcuts(&session, None, ConfigureShortcutsOptions::default()).await.map_err(humanize)
                    } else { open_gnome_shortcut_settings() };
                    let message = match result {
                        Ok(()) => format!("Preferred shortcut saved. Confirm {} in the desktop’s shortcut dialog. In GNOME Settings, open Apps → Spotlight Linux → Global Shortcuts. The Current Shortcut above changes only when the desktop reports the new binding.", super::shortcut_keys::label(preferred)),
                        Err(error) => error,
                    };
                    let _ = events.try_send(ShortcutEvent::Notice { message });
                }
                Ok(ShortcutCommand::Reconnect) => {
                    let _ = session.close().await;
                    // Re-enter immediately without timers or background retries.
                    return Ok(true);
                }
                Ok(ShortcutCommand::Unavailable) => return Err("The desktop portal stopped. Spotlight will reconnect when it returns.".into()),
                Err(_) => { let _ = session.close().await; return Ok(false); }
            },
        }
    }
}

fn open_gnome_shortcut_settings() -> Result<(), String> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if !desktop
        .split(':')
        .any(|part| part.eq_ignore_ascii_case("gnome"))
    {
        return Err("This desktop provides Global Shortcuts version 1. Change Spotlight Linux’s binding in your desktop’s shortcut settings, or upgrade to a desktop supporting version 2. Your existing binding is unchanged.".into());
    }
    let executable = gtk::glib::find_program_in_path("gnome-control-center").ok_or(
        "GNOME Settings is not installed. Install it to change this desktop’s shortcut bindings.",
    )?;
    // A native settings process, with distinct argv and no shell or terminal.
    super::rendering::subprocess_launcher()
        .spawn(&[
            executable.as_os_str(),
            "applications".as_ref(),
            APPLICATION_ID.as_ref(),
        ])
        .map(|_| ())
        .map_err(|e| format!("Could not open GNOME Settings: {e}"))
}

fn is_our_shortcut(incoming: &str, current: &str, id: &str) -> bool {
    incoming == current && id == TOGGLE_SHORTCUT_ID
}

fn trigger_description(shortcuts: &[Shortcut]) -> Option<String> {
    shortcuts
        .iter()
        .find(|s| s.id() == TOGGLE_SHORTCUT_ID)
        .map(|s| s.trigger_description().to_owned())
        .filter(|s| !s.is_empty())
}

fn humanize(error: ashpd::Error) -> String {
    match error {
        ashpd::Error::PortalNotFound(_) => "This desktop does not provide the Global Shortcuts portal. Local search remains available from the application icon. Install a compatible desktop portal, then retry in Keyboard settings.".into(),
        ashpd::Error::Response(_) => "The shortcut was not approved. Open Keyboard settings to retry; local search is still available.".into(),
        _ => format!("Global shortcut unavailable: {error}. Retry from Keyboard settings."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_activations_are_accepted_without_any_release() {
        assert!(!is_our_shortcut("/other", "/ours", TOGGLE_SHORTCUT_ID));
        assert!(!is_our_shortcut("/ours", "/ours", "other"));
        for _ in 0..100 {
            assert!(is_our_shortcut("/ours", "/ours", TOGGLE_SHORTCUT_ID));
        }
    }
}
