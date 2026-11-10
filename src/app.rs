//! La interfaz: una ventana oculta que recibe los clics del ícono, los avisos de Windows y los
//! resultados del hilo del mouse, y lo que se sabe del mouse para mostrarlo. Vive entera en el
//! hilo de la interfaz; con el hilo del mouse solo se intercambian pedidos y resultados.

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
use crate::icon::Rgb;
use crate::menu::{self, Choice, Model};
use crate::tray::{Look, Tray};
use crate::win::{open_url, wide};
use crate::worker::{self, Outcome, Report, Request, Worker};
use crate::{startup, strings, theme};

const WM_TRAY: u32 = WM_APP + 1;
const WM_REPORT: u32 = WM_APP + 2;
/// Enter o espacio sobre el ícono (`NIN_SELECT | NINF_KEY`).
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;

const TIMER_DEVICES: usize = 1;
const TIMER_BATTERY: usize = 2;
/// Conectar algo avisa varias veces seguidas: se pregunta una vez, cuando se calma.
const DEVICES_SETTLE_MS: u32 = 1000;
const BATTERY_EVERY_MS: u32 = 15 * 60 * 1000;
/// Los avisos de batería baja, de mayor a menor.
const LOW_BATTERY: [u8; 2] = [15, 5];

const HUB_URL: &str = "https://hub.atk.pro/";
const CLASS: &str = "PhantomSwitch";

/// El color de cada configuración, y el de no saber cuál es.
const SLOT_COLORS: [Rgb; 4] = [Rgb(37, 99, 235), Rgb(220, 38, 38), Rgb(22, 163, 74), Rgb(217, 119, 6)];
const UNKNOWN_COLOR: Rgb = Rgb(107, 114, 128);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

/// Crea la ventana y el ícono, y atiende mensajes hasta salir.
pub fn run() {
    // SAFETY: llamadas de Win32 con cadenas terminadas en 0 que viven durante cada llamada.
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
        let title = wide(strings::APP_NAME);
        CreateWindowExW(0, class.as_ptr(), title.as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), instance, null())
    };
    if window.is_null() {
        return;
    }
    APP.with_borrow_mut(|app| *app = Some(App::new(window)));
    with_app(App::start);
    // SAFETY: el bucle de mensajes de este hilo.
    unsafe {
        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// `f` con el estado de la interfaz. Nada dentro de `f` puede mandar mensajes a la ventana de
/// forma síncrona (un menú, cerrar la ventana): eso se hace fuera, como en `show_menu`.
fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> Option<T> {
    APP.with(|app| app.try_borrow_mut().ok()?.as_mut().map(f))
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_TRAY => {
            // Versión 4: el evento en la palabra baja de lParam; la posición del ícono en wParam.
            let anchor = POINT { x: i32::from(wparam as u16 as i16), y: i32::from((wparam >> 16) as u16 as i16) };
            with_app(|app| app.on_tray(lparam as u32 & 0xFFFF, anchor));
        }
        WM_REPORT => {
            // SAFETY: este mensaje solo lo envía el hilo del mouse, una vez por resultado.
            let report = unsafe { worker::take_report(lparam) };
            if with_app(|app| app.on_report(*report)) == Some(true) {
                show_menu(window);
            }
        }
        WM_TIMER => {
            let timer = wparam;
            if timer == TIMER_DEVICES {
                // SAFETY: temporizador de esta ventana.
                unsafe { KillTimer(window, TIMER_DEVICES) };
            }
            with_app(|app| app.on_timer(timer));
        }
        WM_DEVICECHANGE if wparam as u32 == DBT_DEVNODES_CHANGED => {
            // SAFETY: temporizador de esta ventana; uno nuevo reemplaza al anterior.
            unsafe { SetTimer(window, TIMER_DEVICES, DEVICES_SETTLE_MS, None) };
        }
        WM_SETTINGCHANGE => theme::refresh(),
        WM_DPICHANGED | WM_DISPLAYCHANGE => {
            with_app(App::show);
        }
        WM_DESTROY => {
            with_app(|app| app.tray.remove());
            // SAFETY: termina el bucle de mensajes de este hilo.
            unsafe { PostQuitMessage(0) };
        }
        _ if message == TASKBAR_CREATED.get() => {
            // El Explorador se reinició: el ícono vuelve a la bandeja.
            with_app(|app| {
                app.tray.forget();
                app.show();
            });
        }
        // SAFETY: lo demás, como lo hace Windows.
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    0
}

/// Abre el menú y atiende lo elegido. El menú atiende mensajes mientras está abierto, así que se
/// muestra sin el estado de la interfaz tomado.
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
        Some(Choice::ToggleStartup) => startup::set_enabled(!model.starts_with_windows),
        Some(Choice::Exit) => {
            // SAFETY: cierra la ventana desde el bucle de mensajes, no desde aquí.
            unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        }
        None => {}
    }
}

/// Lo que se sabe del mouse.
#[derive(Default)]
struct Mouse {
    seen: Seen,
    connection: Option<Connection>,
    slot: Option<Slot>,
    battery: Option<Battery>,
    /// Lo que tenía cada configuración la última vez que estuvo activa.
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
    tray: Tray,
    worker: Worker,
    mouse: Mouse,
    /// La configuración usada antes de la activa: a la que vuelve el clic.
    previous: Option<Slot>,
    /// Un cambio pedido y aún sin respuesta: mientras dura, los clics no piden otro.
    switching: bool,
    /// El aviso de batería baja más bajo ya mostrado en esta descarga.
    warned: Option<u8>,
    /// Dónde abrir el menú que se está leyendo.
    menu_at: POINT,
    menu_open: bool,
}

impl App {
    fn new(window: HWND) -> App {
        App {
            window,
            tray: Tray::new(window, WM_TRAY),
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
        // SAFETY: temporizador de esta ventana.
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
            TIMER_DEVICES => self.worker.send(Request::Refresh),
            TIMER_BATTERY => self.worker.send(Request::Battery),
            _ => {}
        }
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

    /// Aplica un resultado; `true` si hay que abrir el menú.
    fn on_report(&mut self, report: Report) -> bool {
        let switch = matches!(report.request, Request::Back { .. } | Request::Activate(_));
        if switch {
            self.switching = false;
        }
        match report.outcome {
            Outcome::NotFound => {
                self.mouse = Mouse { seen: Seen::NotFound, profiles: self.mouse.profiles, ..Mouse::default() };
                if switch {
                    self.tray.warn(strings::SWITCH_FAILED, strings::NOT_FOUND);
                }
            }
            Outcome::NoAnswer => {
                self.mouse.seen = Seen::NoAnswer;
                self.mouse.slot = None;
                if switch {
                    self.tray.warn(strings::SWITCH_FAILED, strings::NO_ANSWER);
                }
            }
            Outcome::Read { connection, snapshot, switched } => {
                // También cuando se cambió desde ATK HUB: la que estaba activa pasa a ser la anterior.
                if let Some(before) = self.mouse.slot.filter(|before| *before != snapshot.slot) {
                    self.previous = Some(before);
                }
                self.mouse.seen = Seen::Ready;
                self.mouse.connection = Some(connection);
                self.mouse.slot = Some(snapshot.slot);
                self.mouse.battery = snapshot.battery;
                self.mouse.profiles[snapshot.slot.index()] = Some(snapshot.profile);
                if switched == Some(false) {
                    self.tray.warn(strings::SWITCH_FAILED, strings::SWITCH_UNCONFIRMED);
                }
            }
            Outcome::Battery(battery) => self.mouse.battery = Some(battery),
        }
        self.show();
        self.warn_low_battery();
        matches!(report.request, Request::Menu) && !self.menu_open
    }

    fn show(&mut self) {
        let look = self.look();
        self.tray.show(&look);
    }

    fn look(&self) -> Look {
        let mouse = &self.mouse;
        let (label, color) = match mouse.slot {
            Some(slot) => (slot.number().to_string(), SLOT_COLORS[slot.index()]),
            None => ("?".to_owned(), UNKNOWN_COLOR),
        };
        let mut lines = Vec::new();
        match (mouse.seen, mouse.slot) {
            (Seen::Ready, Some(slot)) => {
                lines.push(format!("{} · {}", strings::APP_NAME, strings::slot(slot)));
                lines.extend(strings::connection(mouse.connection, mouse.battery));
                lines.push(strings::click_hint(back_from(slot, self.previous)));
            }
            (seen, _) => {
                lines.push(strings::APP_NAME.to_owned());
                lines.push(
                    match seen {
                        Seen::NotFound => strings::NOT_FOUND,
                        Seen::NoAnswer => strings::NO_ANSWER,
                        Seen::Searching | Seen::Ready => strings::SEARCHING,
                    }
                    .to_owned(),
                );
            }
        }
        Look { label, color, tooltip: lines.join("\n") }
    }

    fn menu_model(&self) -> Model {
        let mouse = &self.mouse;
        Model {
            status: strings::connection(mouse.connection, mouse.battery),
            slots: Slot::ALL
                .map(|slot| (slot, mouse.profiles[slot.index()].map(|p| strings::profile(&p)).unwrap_or_default())),
            active: mouse.slot,
            starts_with_windows: startup::is_enabled(),
        }
    }

    fn warn_low_battery(&mut self) {
        let (warned, notice) = low_battery_notice(self.mouse.battery, self.warned);
        self.warned = warned;
        if let (Some(threshold), Some(battery)) = (notice, self.mouse.battery) {
            let (title, body) = strings::low_battery(threshold, battery.percent);
            self.tray.warn(&title, body);
        }
    }
}

/// A la que vuelve el clic estando en `active`: la usada antes, o la 1 o la 2 mientras no la hay.
pub fn back_from(active: Slot, previous: Option<Slot>) -> Slot {
    let [first, second, ..] = Slot::ALL;
    previous.filter(|p| *p != active).unwrap_or(if active == first { second } else { first })
}

/// El aviso que toca con esta batería: (el aviso más bajo ya mostrado, el que hay que mostrar).
/// Cada umbral avisa una vez por descarga; cargar o recuperarse los reinicia.
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
