//! What the user chooses, kept in their registry (no files beside the `.exe`): whether the app
//! starts with Windows (`HKCU\…\Run`) and whether it shows the on-screen notice
//! (`HKCU\Software\PhantomSwitch`).

use crate::{APP_ID, registry};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const SHOW_OVERLAY: &str = "ShowOverlay";

/// Whether Windows starts this very `.exe` at sign-in (an entry left by a copy that was moved
/// does not count).
pub fn starts_with_windows() -> bool {
    command().is_some_and(|command| registry::string(RUN_KEY, APP_ID).as_deref() == Some(command.as_str()))
}

pub fn set_starts_with_windows(enabled: bool) {
    if !enabled {
        registry::delete(RUN_KEY, APP_ID);
    } else if let Some(command) = command() {
        registry::set_string(RUN_KEY, APP_ID, &command);
    }
}

/// Whether to show the on-screen notice when switching; on until the user turns it off.
pub fn shows_overlay() -> bool {
    registry::dword(&app_key(), SHOW_OVERLAY).is_none_or(|shows| shows != 0)
}

pub fn set_shows_overlay(enabled: bool) {
    registry::set_dword(&app_key(), SHOW_OVERLAY, u32::from(enabled));
}

/// How Windows is told to start this `.exe`.
fn command() -> Option<String> {
    std::env::current_exe().ok().map(|exe| format!("\"{}\"", exe.display()))
}

fn app_key() -> String {
    format!(r"Software\{APP_ID}")
}
