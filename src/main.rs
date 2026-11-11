//! Phantom Switch: switches the onboard profiles of the ATK Phantom Ultimate from an icon next
//! to the Windows clock, with the same commands ATK HUB uses.
//!
//! - `device`: the mouse (its protocol apart from Windows' I/O, so it can be tested).
//! - `worker`: the thread that talks to the mouse; the interface never waits for it.
//! - `app`: the interface and what is known of the mouse; `tray`, `menu` and `overlay` show it,
//!   all drawn with `paint` and `badge`.
//!
//! It never listens to the mouse or the keyboard: it talks to the mouse once at start, when asked,
//! when Windows says a device was plugged in or out, and, through the receiver, for the battery
//! every 15 minutes. Once a day at most, `update` asks GitHub for the latest version's number.

#![windows_subsystem = "windows"]

/// The app's name, as the user reads it.
pub const APP_NAME: &str = "Phantom Switch";
/// The app's name where Windows needs an identifier: its window classes, its single-instance
/// mutex and its registry entries.
pub const APP_ID: &str = "PhantomSwitch";

mod app;
mod badge;
mod device;
mod menu;
mod overlay;
mod paint;
mod registry;
mod settings;
mod strings;
mod theme;
mod tray;
mod update;
mod win;
mod worker;

fn main() {
    let Some(_instance) = win::SingleInstance::acquire(APP_ID) else {
        win::message(strings::ui().already_running());
        return;
    };
    theme::follow_system();
    app::run();
}
