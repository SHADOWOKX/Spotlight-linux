use gio_unix::DesktopAppInfo;
use gtk::{gdk, prelude::*};
use spotlight_core::Action;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionOutcome {
    Launched,
    Copied,
    OpenSettings,
}

pub fn execute(action: &Action) -> Result<ActionOutcome, String> {
    match action {
        Action::LaunchDesktopEntry { desktop_id } => {
            let application = DesktopAppInfo::new(desktop_id)
                .ok_or_else(|| format!("Application ‘{desktop_id}’ is no longer installed"))?;
            let context = crate::platform::rendering::launch_context();
            application
                .launch(&[], Some(&context))
                .map_err(|error| format!("Could not launch {}: {error}", application.name()))?;
            Ok(ActionOutcome::Launched)
        }
        Action::LaunchDesktopAction {
            desktop_id,
            action_id,
        } => {
            let application = DesktopAppInfo::new(desktop_id)
                .ok_or_else(|| format!("Application ‘{desktop_id}’ is no longer installed"))?;
            if !application
                .list_actions()
                .iter()
                .any(|available| available.as_str() == action_id)
            {
                return Err("That application action is no longer available".into());
            }
            let context = crate::platform::rendering::launch_context();
            application.launch_action(action_id, Some(&context));
            Ok(ActionOutcome::Launched)
        }
        Action::OpenSettings => Ok(ActionOutcome::OpenSettings),
        Action::CopyText { text } => {
            let display = gdk::Display::default()
                .ok_or_else(|| "No display is available for clipboard access".to_owned())?;
            display.clipboard().set_text(text);
            Ok(ActionOutcome::Copied)
        }
    }
}
