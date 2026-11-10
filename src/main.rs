//! Phantom Switch: switches the onboard profiles of the ATK Phantom Ultimate from an icon next
//! to the Windows clock, with the same commands ATK HUB uses.
//!
//! - `device`: the mouse (its protocol apart from Windows' I/O, so it can be tested).
//! - `worker`: the thread that talks to the mouse; the interface never waits for it.
//! - `app`: the interface and what is known of the mouse; `tray`, `menu` and `overlay` show it,
//!   all drawn with `paint` and `badge`.
//!
//! It never listens to the mouse or the keyboard: it talks to the mouse only when asked, when
//! Windows says something was plugged in or out, and, through the receiver, for the battery every
//! 15 minutes.

#![windows_subsystem = "windows"]

mod app;
mod badge;
mod device;
mod menu;
mod overlay;
mod paint;
mod settings;
mod strings;
mod theme;
mod tray;
mod win;
mod worker;

fn main() {
    let Some(_instance) = win::SingleInstance::acquire("PhantomSwitch") else {
        win::message(strings::ui().already_running());
        return;
    };
    theme::follow_system();
    app::run();
}
