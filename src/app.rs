//! The interface: a hidden window that receives the tray icon's clicks, Windows' notices and the
//! mouse thread's results, and what is known of the mouse to show it. It all lives on the
//! interface's thread; with the mouse thread it only trades requests and results.

use std::cell::{Cell, RefCell};
use std::mem::zeroed;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::NIN_SELECT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DBT_DEVNODES_CHANGED, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, MSG, PostMessageW,
    PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetTimer, TranslateMessage, WM_APP, WM_CLOSE,
    WM_CONTEXTMENU, WM_DESTROY, WM_DEVICECHANGE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_SETTINGCHANGE, WM_TIMER,
    WNDCLASSW,
};

use crate::device::{Battery, Connection, Profile, Slot};
use crate::menu::{self, Choice, Model};
use crate::overlay::Overlay;
use crate::strings::{self, APP_NAME, Lang};
use crate::tray::Tray;
use crate::win::{open_url, wide};
use crate::worker::{self, Outcome, Report, Request, Worker};
use crate::{settings, theme};

const WM_TRAY: u32 = WM_APP + 1;
const WM_REPORT: u32 = WM_APP + 2;
/// Enter or Space on the icon (`NIN_SELECT | NINF_KEY`).
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;

const TIMER_DEVICES: usize = 1;
const TIMER_BATTERY: usize = 2;
const TIMER_OVERLAY: usize = 3;
/// Plugging something in sends several notices in a row: ask once, when they settle.
const DEVICES_SETTLE_MS: u32 = 1000;
const BATTERY_EVERY_MS: u32 = 15 * 60 * 1000;
/// The low-battery warnings, highest first.
const LOW_BATTERY: [u8; 2] = [15, 5];

const HUB_URL: &str = "https://hub.atk.pro/";
const CLASS: &str = "PhantomSwitch";

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

/// Creates the window and the icon, and handles messages until exit.
pub fn run() {
    // SAFETY: Win32 calls with NUL-terminated strings that live through each call.
    let window = unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide(CLASS);
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..zeroed()
        };
        RegisterClassW(&window_class);
        TASKBAR_CREATED.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));
        let title = wide(APP_NAME);
        CreateWindowExW(0, class.as_ptr(), title.as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), instance, null())
    };
    if window.is_null() {
        return;
    }
    APP.with_borrow_mut(|app| *app = Some(App::new(window)));
    with_app(App::start);
    // SAFETY: this thread's message loop.
    unsafe {
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// `f` with the interface's state. Nothing inside `f` may send messages to the window
/// synchronously (a menu, closing the window): that is done outside, as `show_menu` does.
fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> Option<T> {
    APP.with(|app| app.try_borrow_mut().ok()?.as_mut().map(f))
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_TRAY => {
            // Version 4: the event in lParam's low word, the icon's position in wParam.
            let anchor = POINT { x: i32::from(wparam as u16 as i16), y: i32::from((wparam >> 16) as u16 as i16) };
            with_app(|app| app.on_tray(lparam as u32 & 0xFFFF, anchor));
        }
        WM_REPORT => {
            // SAFETY: only the mouse thread sends this message, once per result.
            let report = unsafe { worker::take_report(lparam) };
            if with_app(|app| app.on_report(*report)) == Some(true) {
                show_menu(window);
            }
        }
        WM_TIMER => {
            with_app(|app| app.on_timer(wparam));
        }
        WM_DEVICECHANGE if wparam as u32 == DBT_DEVNODES_CHANGED => {
            // SAFETY: this window's timer; a new one replaces the previous.
            unsafe { SetTimer(window, TIMER_DEVICES, DEVICES_SETTLE_MS, None) };
        }
        WM_SETTINGCHANGE => theme::refresh(),
        WM_DPICHANGED | WM_DISPLAYCHANGE => {
            with_app(App::show);
        }
        WM_DESTROY => {
            with_app(|app| app.tray.remove());
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
        }
        _ if message == TASKBAR_CREATED.get() => {
            // Explorer restarted: the icon goes back to the tray.
            with_app(|app| {
                app.tray.forget();
                app.show();
            });
        }
        // SAFETY: everything else, as Windows does it.
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    0
}

/// Opens the menu and acts on the choice. The menu handles messages while it is open, so it is
/// shown without the interface's state borrowed.
fn show_menu(window: HWND) {
    let Some((model, at)) = with_app(|app| {
        app.menu_open = true;
        (app.menu_model(), app.menu_at)
    }) else {
        return;
    };
    let choice = menu::show(window, &model, at);
    with_app(|app| app.menu_open = false);
    match choice {
        Some(Choice::Activate(slot)) => {
            with_app(|app| app.activate(slot));
        }
        Some(Choice::OpenHub) => open_url(HUB_URL),
        Some(Choice::ToggleOverlay) => settings::set_shows_overlay(!model.shows_overlay),
        Some(Choice::ToggleStartup) => settings::set_starts_with_windows(!model.starts_with_windows),
        Some(Choice::Exit) => {
            // SAFETY: closes the window from the message loop, not from here.
            unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        }
        None => {}
    }
}

/// What is known of the mouse.
#[derive(Default)]
struct Mouse {
    seen: Seen,
    connection: Option<Connection>,
    slot: Option<Slot>,
    battery: Option<Battery>,
    /// What each profile had the last time it was active.
    profiles: [Option<Profile>; 4],
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Seen {
    #[default]
    Searching,
    Ready,
    NotFound,
    NoAnswer,
}

struct App {
    window: HWND,
    lang: Lang,
    tray: Tray,
    overlay: Option<Overlay>,
    worker: Worker,
    mouse: Mouse,
    /// The profile used before the active one: where the click goes back to.
    previous: Option<Slot>,
    /// A switch asked for and not answered yet: meanwhile, clicks ask for no other.
    switching: bool,
    /// The lowest low-battery warning already shown in this discharge.
    warned: Option<u8>,
    /// Where to open the menu being read.
    menu_at: POINT,
    menu_open: bool,
}

impl App {
    fn new(window: HWND) -> App {
        App {
            window,
            lang: strings::ui(),
            tray: Tray::new(window, WM_TRAY),
            overlay: Overlay::new(),
            worker: Worker::spawn(window, WM_REPORT),
            mouse: Mouse::default(),
            previous: None,
            switching: false,
            warned: None,
            menu_at: POINT { x: 0, y: 0 },
            menu_open: false,
        }
    }

    fn start(&mut self) {
        self.show();
        self.worker.send(Request::Refresh);
        // SAFETY: this window's timer.
        unsafe { SetTimer(self.window, TIMER_BATTERY, BATTERY_EVERY_MS, None) };
    }

    fn on_tray(&mut self, event: u32, anchor: POINT) {
        match event {
            NIN_SELECT | NIN_KEYSELECT => self.switch(Request::Back { previous: self.previous }),
            WM_CONTEXTMENU if !self.menu_open => {
                self.menu_at = anchor;
                self.worker.send(Request::Menu);
            }
            _ => {}
        }
    }

    fn on_timer(&mut self, timer: usize) {
        match timer {
            TIMER_DEVICES => {
                self.kill_timer(TIMER_DEVICES);
                self.worker.send(Request::Refresh);
            }
            TIMER_BATTERY => self.worker.send(Request::Battery),
            // The notice is over: no more frames.
            TIMER_OVERLAY if !self.overlay.as_mut().is_some_and(Overlay::tick) => self.kill_timer(TIMER_OVERLAY),
            _ => {}
        }
    }

    fn kill_timer(&self, timer: usize) {
        // SAFETY: this window's timer.
        unsafe { KillTimer(self.window, timer) };
    }

    fn activate(&mut self, slot: Slot) {
        if self.mouse.slot != Some(slot) {
            self.switch(Request::Activate(slot));
        }
    }

    fn switch(&mut self, request: Request) {
        if !self.switching {
            self.switching = true;
            self.worker.send(request);
        }
    }

    /// Applies a result; `true` if the menu must open.
    fn on_report(&mut self, report: Report) -> bool {
        let lang = self.lang;
        let switch = matches!(report.request, Request::Back { .. } | Request::Activate(_));
        if switch {
            self.switching = false;
        }
        match report.outcome {
            Outcome::NotFound => {
                self.mouse = Mouse { seen: Seen::NotFound, profiles: self.mouse.profiles, ..Mouse::default() };
                if switch {
                    self.tray.warn(lang.switch_failed(), lang.not_found());
                }
            }
            Outcome::NoAnswer => {
                self.mouse.seen = Seen::NoAnswer;
                self.mouse.slot = None;
                if switch {
                    self.tray.warn(lang.switch_failed(), lang.no_answer());
                }
            }
            Outcome::Read { connection, snapshot, switched } => {
                // Also when it was switched from ATK HUB: the one that was active becomes the previous.
                if let Some(before) = self.mouse.slot.filter(|before| *before != snapshot.slot) {
                    self.previous = Some(before);
                }
                self.mouse.seen = Seen::Ready;
                self.mouse.connection = Some(connection);
                self.mouse.slot = Some(snapshot.slot);
                self.mouse.battery = snapshot.battery;
                self.mouse.profiles[snapshot.slot.index()] = Some(snapshot.profile);
                match switched {
                    Some(true) => self.announce(snapshot.slot, &snapshot.profile),
                    Some(false) => self.tray.warn(lang.switch_failed(), lang.switch_unconfirmed()),
                    None => {}
                }
            }
            Outcome::Battery(battery) => self.mouse.battery = Some(battery),
        }
        self.show();
        self.warn_low_battery();
        matches!(report.request, Request::Menu) && !self.menu_open
    }

    /// The on-screen notice of a switch, unless the user turned it off.
    fn announce(&mut self, slot: Slot, profile: &Profile) {
        if !settings::shows_overlay() {
            return;
        }
        let (title, details) = (self.lang.slot(slot), self.lang.profile(profile));
        if let Some(every) = self.overlay.as_mut().map(|overlay| overlay.show(slot, &title, &details)) {
            // SAFETY: this window's timer; a new one replaces the previous.
            unsafe { SetTimer(self.window, TIMER_OVERLAY, every, None) };
        }
    }

    fn show(&mut self) {
        let tooltip = self.tooltip();
        self.tray.show(self.mouse.slot, &tooltip);
    }

    fn tooltip(&self) -> String {
        let (lang, mouse) = (self.lang, &self.mouse);
        let mut lines = vec![APP_NAME.to_owned()];
        match (mouse.seen, mouse.slot) {
            (Seen::Ready, Some(slot)) => {
                lines[0] = format!("{APP_NAME} · {}", lang.slot(slot));
                lines.extend(lang.connection(mouse.connection, mouse.battery));
                lines.push(lang.click_hint(back_from(slot, self.previous)));
            }
            (Seen::NotFound, _) => lines.push(lang.not_found().to_owned()),
            (Seen::NoAnswer, _) => lines.push(lang.no_answer().to_owned()),
            (Seen::Searching | Seen::Ready, _) => lines.push(lang.searching().to_owned()),
        }
        lines.join("\n")
    }

    fn menu_model(&self) -> Model {
        let (lang, mouse) = (self.lang, &self.mouse);
        Model {
            lang,
            status: lang.connection(mouse.connection, mouse.battery),
            slots: Slot::ALL
                .map(|slot| (slot, mouse.profiles[slot.index()].map(|p| lang.profile(&p)).unwrap_or_default())),
            active: mouse.slot,
            shows_overlay: settings::shows_overlay(),
            starts_with_windows: settings::starts_with_windows(),
            badge_size: self.tray.icon_size(),
        }
    }

    fn warn_low_battery(&mut self) {
        let (warned, notice) = low_battery_notice(self.mouse.battery, self.warned);
        self.warned = warned;
        if let (Some(threshold), Some(battery)) = (notice, self.mouse.battery) {
            let (title, body) = self.lang.low_battery(threshold, battery.percent);
            self.tray.warn(&title, body);
        }
    }
}

/// Where the click goes from `active`: the one used before, or the 1 or the 2 while there is none.
pub fn back_from(active: Slot, previous: Option<Slot>) -> Slot {
    let [first, second, ..] = Slot::ALL;
    previous.filter(|p| *p != active).unwrap_or(if active == first { second } else { first })
}

/// The warning due with this battery: (the lowest warning already shown, the one to show).
/// Each threshold warns once per discharge; charging or recovering starts them over.
fn low_battery_notice(battery: Option<Battery>, warned: Option<u8>) -> (Option<u8>, Option<u8>) {
    let Some(Battery { percent, charging }) = battery else { return (warned, None) };
    if charging || percent > LOW_BATTERY[0] {
        return (None, None);
    }
    match LOW_BATTERY.iter().copied().filter(|t| percent <= *t).min() {
        Some(threshold) if warned.is_none_or(|w| threshold < w) => (Some(threshold), Some(threshold)),
        _ => (warned, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(number: u8) -> Slot {
        Slot::ALL[usize::from(number - 1)]
    }

    fn battery(percent: u8, charging: bool) -> Option<Battery> {
        Some(Battery { percent, charging })
    }

    #[test]
    fn the_click_goes_back_to_the_one_used_before() {
        assert_eq!(back_from(slot(2), Some(slot(1))), slot(1));
        assert_eq!(back_from(slot(1), Some(slot(3))), slot(3));
        assert_eq!(back_from(slot(3), Some(slot(2))), slot(2));
    }

    #[test]
    fn without_one_before_it_alternates_the_first_two() {
        assert_eq!(back_from(slot(1), None), slot(2));
        assert_eq!(back_from(slot(2), None), slot(1));
        assert_eq!(back_from(slot(4), None), slot(1));
        assert_eq!(back_from(slot(2), Some(slot(2))), slot(1));
    }

    #[test]
    fn warns_once_below_15_and_again_below_5() {
        let mut warned = None;
        let mut notices = Vec::new();
        for percent in [40, 16, 15, 14, 12, 6, 5, 4, 3] {
            let (now, notice) = low_battery_notice(battery(percent, false), warned);
            warned = now;
            notices.extend(notice);
        }
        assert_eq!(notices, [15, 5]);
    }

    #[test]
    fn charging_starts_the_warnings_again() {
        let (warned, _) = low_battery_notice(battery(4, false), None);
        assert_eq!(low_battery_notice(battery(4, true), warned), (None, None));
        assert_eq!(low_battery_notice(battery(14, false), None), (Some(15), Some(15)));
    }

    #[test]
    fn a_first_reading_at_3_warns_only_the_last_one() {
        assert_eq!(low_battery_notice(battery(3, false), None), (Some(5), Some(5)));
    }
}
