//! The interface: a hidden window that receives the tray icon's clicks, Windows' notices and the
//! mouse thread's results, and what is known of the mouse to show it. It all lives on the
//! interface's thread; with the mouse thread it only trades requests and results, and the check
//! for a new version and its install answer it the same way.

use std::cell::{Cell, RefCell};
use std::mem::zeroed;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{NIN_SELECT, NINF_KEY};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DBT_DEVNODES_CHANGED, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, MSG, PostMessageW,
    PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SetTimer, TranslateMessage, WM_APP, WM_CLOSE,
    WM_CONTEXTMENU, WM_DESTROY, WM_DEVICECHANGE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_SETTINGCHANGE, WM_TIMER,
    WNDCLASSW,
};

use crate::device::{Battery, Connection, Profile, Slot};
use crate::menu::{self, Choice, Model, Offer};
use crate::overlay::Overlay;
use crate::strings::{self, Lang};
use crate::tray::Tray;
use crate::update::install;
use crate::update::{self, Answer, Version};
use crate::win::{open_url, wide};
use crate::worker::{self, Job, Outcome, Reply, Worker, back_from};
use crate::{APP_ID, APP_NAME, settings, theme};

const WM_TRAY: u32 = WM_APP + 1;
const WM_REPLY: u32 = WM_APP + 2;
const WM_UPDATE: u32 = WM_APP + 3;
const WM_INSTALLED: u32 = WM_APP + 4;
/// Enter or Space on the icon.
const NIN_KEYSELECT: u32 = NIN_SELECT | NINF_KEY;

const TIMER_DEVICES: usize = 1;
const TIMER_BATTERY: usize = 2;
const TIMER_OVERLAY: usize = 3;
/// Plugging something in sends several notices in a row: ask once, when they settle.
const DEVICES_SETTLE_MS: u32 = 1000;
const BATTERY_EVERY_MS: u32 = 15 * 60 * 1000;
/// The low-battery warnings, highest first.
const LOW_BATTERY: [u8; 2] = [15, 5];

const HUB_URL: &str = "https://hub.atk.pro/";

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

/// Creates the window and the icon, and handles messages until exit; `updated`, when this start
/// follows an update.
pub fn run(updated: bool) {
    // SAFETY: Win32 calls with NUL-terminated strings that live through each call.
    let window = unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide(APP_ID);
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
    with_app(|app| app.start(updated));
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
        WM_REPLY => {
            // SAFETY: only the mouse thread sends this message, once per reply.
            let reply = unsafe { worker::take_reply(lparam) };
            if with_app(|app| app.on_reply(*reply)) == Some(true) {
                show_menu(window);
            }
        }
        WM_UPDATE => {
            with_app(|app| app.on_update(update::take_answer(wparam, lparam)));
        }
        WM_INSTALLED => {
            with_app(|app| app.on_installed(install::take_outcome(wparam)));
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
            with_app(|app| app.switch(Job::Activate(slot)));
        }
        Some(Choice::Download) => open_url(update::RELEASE_PAGE),
        Some(Choice::Install) => {
            with_app(App::install);
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
    battery: Option<Battery>,
    /// What each profile had the last time it was active.
    profiles: [Option<Profile>; Slot::ALL.len()],
}

impl Mouse {
    /// The active profile, while the mouse answers.
    fn slot(&self) -> Option<Slot> {
        match self.seen {
            Seen::Ready(slot) => Some(slot),
            Seen::Searching | Seen::NotFound | Seen::NoAnswer => None,
        }
    }
}

/// Where installing a new version stands.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Install {
    Idle,
    Busy,
    /// In place: there is nothing more to offer until the app opens again.
    Done,
}

#[derive(Default, Clone, Copy)]
enum Seen {
    #[default]
    Searching,
    /// It answered, with this profile active.
    Ready(Slot),
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
    /// The latest release known, from the last check that got an answer.
    latest: Option<Version>,
    /// A check for a new version asked for and not answered yet.
    checking: bool,
    /// When the last check could not ask, in seconds since 1970: the next one waits a while.
    check_failed: Option<u64>,
    install: Install,
}

impl App {
    fn new(window: HWND) -> App {
        App {
            window,
            lang: strings::ui(),
            tray: Tray::new(window, WM_TRAY),
            overlay: Overlay::new(),
            worker: Worker::spawn(window, WM_REPLY),
            mouse: Mouse::default(),
            previous: None,
            switching: false,
            warned: None,
            menu_at: POINT { x: 0, y: 0 },
            menu_open: false,
            latest: settings::latest_version(),
            checking: false,
            check_failed: None,
            install: Install::Idle,
        }
    }

    fn start(&mut self, updated: bool) {
        self.show();
        if updated {
            self.tray.tell(self.lang.updated(), &self.lang.now_version(Version::current()));
        }
        self.worker.send(Job::Refresh);
        // SAFETY: this window's timer.
        unsafe { SetTimer(self.window, TIMER_BATTERY, BATTERY_EVERY_MS, None) };
        self.check_for_update();
    }

    fn on_tray(&mut self, event: u32, anchor: POINT) {
        match event {
            NIN_SELECT | NIN_KEYSELECT => self.switch(Job::Back { previous: self.previous }),
            WM_CONTEXTMENU if !self.menu_open => {
                self.menu_at = anchor;
                self.worker.send(Job::Menu);
            }
            _ => {}
        }
    }

    fn on_timer(&mut self, timer: usize) {
        match timer {
            TIMER_DEVICES => {
                self.kill_timer(TIMER_DEVICES);
                self.worker.send(Job::Refresh);
            }
            TIMER_BATTERY => {
                self.worker.send(Job::Battery);
                // No timer of its own: it rides on the battery's, and asks only when a day has passed.
                self.check_for_update();
            }
            // The notice is over: no more frames.
            TIMER_OVERLAY if !self.overlay.as_mut().is_some_and(Overlay::tick) => self.kill_timer(TIMER_OVERLAY),
            _ => {}
        }
    }

    fn check_for_update(&mut self) {
        if !self.checking && update::due(settings::update_checked(), self.check_failed, update::now()) {
            self.checking = true;
            update::check_in_background(self.window, WM_UPDATE);
        }
    }

    /// Keeps what the check found, or when it could not ask, to try again later.
    fn on_update(&mut self, answer: Answer) {
        self.checking = false;
        match answer {
            Ok(latest) => {
                settings::set_update_checked(update::now(), latest);
                self.latest = latest;
                self.check_failed = None;
            }
            Err(_) => self.check_failed = Some(update::now()),
        }
    }

    /// Installs the newer release the menu offers, on a thread of its own.
    fn install(&mut self) {
        if let (Install::Idle, Some(version)) = (self.install, self.newer()) {
            self.install = Install::Busy;
            install::in_background(self.window, WM_INSTALLED, version);
        }
    }

    fn on_installed(&mut self, outcome: install::Outcome) {
        let lang = self.lang;
        self.install = Install::Idle;
        match outcome {
            install::Outcome::Restarting => {
                self.install = Install::Done;
                // The new copy takes the tray once this one closes.
                // SAFETY: closes the window from the message loop, not from here.
                unsafe { PostMessageW(self.window, WM_CLOSE, 0, 0) };
            }
            install::Outcome::Installed => {
                self.install = Install::Done;
                self.tray.tell(lang.updated(), lang.next_time());
            }
            install::Outcome::Unreachable => self.tray.warn(lang.update_failed(), lang.update_unreachable()),
            install::Outcome::NotAuthentic => self.tray.warn(lang.update_failed(), lang.update_not_authentic()),
            install::Outcome::ByHand => {
                open_url(update::RELEASE_PAGE);
                self.tray.warn(lang.update_failed(), lang.update_by_hand());
            }
        }
    }

    /// The latest release known, when it is newer than this one.
    fn newer(&self) -> Option<Version> {
        self.latest.filter(|latest| latest.is_newer())
    }

    fn kill_timer(&self, timer: usize) {
        // SAFETY: this window's timer.
        unsafe { KillTimer(self.window, timer) };
    }

    fn switch(&mut self, job: Job) {
        if !self.switching {
            self.switching = true;
            self.worker.send(job);
        }
    }

    /// Applies a reply; `true` if the menu must open.
    fn on_reply(&mut self, reply: Reply) -> bool {
        let lang = self.lang;
        let switch = reply.job.switches();
        if switch {
            self.switching = false;
        }
        match reply.outcome {
            Outcome::NotFound => {
                self.mouse = Mouse { seen: Seen::NotFound, profiles: self.mouse.profiles, ..Mouse::default() };
                if switch {
                    self.tray.warn(lang.switch_failed(), lang.not_found());
                }
            }
            Outcome::NoAnswer => {
                self.mouse.seen = Seen::NoAnswer;
                if switch {
                    self.tray.warn(lang.switch_failed(), lang.no_answer());
                }
            }
            Outcome::Read { connection, snapshot, switched } => {
                // Also when it was switched from ATK HUB: the one that was active becomes the previous.
                if let Some(before) = self.mouse.slot().filter(|before| *before != snapshot.slot) {
                    self.previous = Some(before);
                }
                self.mouse.seen = Seen::Ready(snapshot.slot);
                self.mouse.connection = Some(connection);
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
        matches!(reply.job, Job::Menu) && !self.menu_open
    }

    /// The on-screen notice of a switch, unless the user turned it off.
    fn announce(&mut self, slot: Slot, profile: &Profile) {
        if !settings::shows_overlay() {
            return;
        }
        let (title, details) = (self.lang.profile_name(slot), self.lang.profile_details(profile));
        if let Some(every) = self.overlay.as_mut().map(|overlay| overlay.show(slot, &title, &details)) {
            // SAFETY: this window's timer; a new one replaces the previous.
            unsafe { SetTimer(self.window, TIMER_OVERLAY, every, None) };
        }
    }

    fn show(&mut self) {
        let tooltip = self.tooltip();
        self.tray.show(self.mouse.slot(), &tooltip);
    }

    fn tooltip(&self) -> String {
        let (lang, mouse) = (self.lang, &self.mouse);
        let mut lines = vec![APP_NAME.to_owned()];
        match mouse.seen {
            Seen::Ready(slot) => {
                lines[0] = format!("{APP_NAME} · {}", lang.profile_name(slot));
                lines.extend(lang.status(mouse.connection, mouse.battery));
                lines.push(lang.click_hint(back_from(slot, self.previous)));
            }
            Seen::Searching => lines.push(lang.searching().to_owned()),
            Seen::NotFound => lines.push(lang.not_found().to_owned()),
            Seen::NoAnswer => lines.push(lang.no_answer().to_owned()),
        }
        lines.join("\n")
    }

    fn menu_model(&self) -> Model {
        let (lang, mouse) = (self.lang, &self.mouse);
        Model {
            lang,
            status: lang.status(mouse.connection, mouse.battery),
            slots: Slot::ALL
                .map(|slot| (slot, mouse.profiles[slot.index()].map(|p| lang.profile_details(&p)).unwrap_or_default())),
            active: mouse.slot(),
            shows_overlay: settings::shows_overlay(),
            starts_with_windows: settings::starts_with_windows(),
            update: self.newer().and_then(|version| match self.install {
                Install::Idle if install::can_install() => Some(Offer::Install(version)),
                Install::Idle => Some(Offer::Download(version)),
                Install::Busy => Some(Offer::Installing(version)),
                Install::Done => None,
            }),
            badge_size: self.tray.icon_size(),
        }
    }

    fn warn_low_battery(&mut self) {
        let (warned, notice) = low_battery_notice(self.mouse.battery, self.warned);
        self.warned = warned;
        if let (Some(threshold), Some(battery)) = (notice, self.mouse.battery) {
            let critical = Some(&threshold) == LOW_BATTERY.last();
            let (title, body) = self.lang.low_battery(critical, battery.percent);
            self.tray.warn(&title, body);
        }
    }
}

/// The warning due with this battery: (the lowest warning already shown, the one to show).
/// Each threshold warns once per discharge, below it; charging or recovering starts them over.
fn low_battery_notice(battery: Option<Battery>, warned: Option<u8>) -> (Option<u8>, Option<u8>) {
    let Some(Battery { percent, charging }) = battery else { return (warned, None) };
    if charging || percent >= LOW_BATTERY[0] {
        return (None, None);
    }
    match LOW_BATTERY.iter().copied().filter(|t| percent < *t).min() {
        Some(threshold) if warned.is_none_or(|w| threshold < w) => (Some(threshold), Some(threshold)),
        _ => (warned, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery(percent: u8, charging: bool) -> Option<Battery> {
        Some(Battery { percent, charging })
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
    fn at_a_threshold_it_does_not_warn_yet() {
        assert_eq!(low_battery_notice(battery(15, false), None), (None, None));
        assert_eq!(low_battery_notice(battery(5, false), Some(15)), (Some(15), None));
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
