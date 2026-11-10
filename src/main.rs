//! ATK Phantom Modo: cambia la configuración interna del ATK Phantom Ultimate (1 a 4) desde un
//! ícono junto al reloj. Clic izquierdo: vuelve a la configuración que usaste antes (la 1 o la 2
//! mientras no hayas usado otra). Clic derecho: el menú con las cuatro, con la batería y el
//! polling rate, el DPI y el modo del sensor de cada una, leídos al abrirlo.
//! No escucha el mouse: solo le habla cuando se lo pides, cuando Windows avisa que se conectó o
//! desconectó algo y, por el receptor, para revisar la batería cada 15 minutos (avisa al bajar
//! del 15 % y del 5 %).

#![windows_subsystem = "windows"]

mod hid;
mod icon;

use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    ShellExecuteW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DBT_DEVNODES_CHANGED, DefWindowProcW, DestroyIcon, DestroyMenu,
    DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, HICON, KillTimer, MB_ICONINFORMATION, MB_OK,
    MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SW_SHOWNORMAL, SetForegroundWindow, SetTimer, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, WM_APP, WM_DESTROY, WM_DEVICECHANGE, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
};

use hid::{Gone, Phantom, Sensor, Snapshot};

const WM_TRAY: u32 = WM_APP + 1;
const WM_RESULT: u32 = WM_APP + 2;
const WM_MENU: u32 = WM_APP + 3;
const TIMER_DEVICES: usize = 1;
const TIMER_BATTERY: usize = 2;
const BATTERY_EVERY_MS: u32 = 15 * 60 * 1000;
/// Avisos de batería baja, de mayor a menor.
const LOW_BATTERY: [u8; 2] = [15, 5];
/// Las configuraciones son `MENU_CONFIG + 1` a `MENU_CONFIG + 4`.
const MENU_CONFIG: usize = 10;
const MENU_HUB: usize = 19;
const MENU_STARTUP: usize = 20;
const MENU_EXIT: usize = 30;
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "AtkPhantomModo";

/// Lo que pide la interfaz al hilo que habla con el mouse.
enum Command {
    Refresh,
    /// Lee el mouse y luego abre el menú.
    Menu,
    /// Vuelve a la configuración usada antes.
    Toggle,
    Set(u8),
    /// Solo la batería, y solo por el receptor: sin el mouse a mano, no hace nada.
    Battery,
}

/// Cómo terminó el último pedido, para el texto del ícono.
#[derive(Clone, Copy, PartialEq)]
enum Status {
    Searching,
    Ok,
    NotFound,
    NotConfirmed,
}

/// Polling rate, DPI (horizontal, vertical) y modo del sensor de una configuración.
#[derive(Clone, Copy)]
struct Mode {
    rate_hz: Option<u32>,
    dpi: Option<(u16, u16)>,
    sensor: Option<Sensor>,
}

/// Lo que se sabe del mouse: lo que muestran el ícono y el menú.
struct State {
    config: Option<u8>,
    status: Status,
    battery: Option<(u8, bool)>,
    /// Si el mouse está conectado por el receptor (si no, por cable).
    wireless: Option<bool>,
    /// El aviso de batería baja más bajo ya mostrado en esta descarga.
    warned: Option<u8>,
    /// La configuración usada antes de la activa: a la que vuelve el clic.
    previous: Option<u8>,
    /// Lo que se leyó de cada configuración la última vez que estuvo activa.
    modes: [Option<Mode>; 4],
}

static WORKER: OnceLock<Sender<Command>> = OnceLock::new();
static WINDOW: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut());
/// Un cambio pedido y aún sin respuesta: mientras dura, los clics no piden otro.
static CHANGING: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> =
    Mutex::new(State {
        config: None,
        status: Status::Searching,
        battery: None,
        wireless: None,
        warned: None,
        previous: None,
        modes: [None; 4],
    });
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn send(command: Command) {
    let change = matches!(command, Command::Toggle | Command::Set(_));
    if change && CHANGING.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(worker) = WORKER.get() {
        let _ = worker.send(command);
    }
}

fn main() {
    // SAFETY: llamadas a Win32 con cadenas terminadas en 0 y estructuras inicializadas.
    unsafe {
        CreateMutexW(null(), 1, wide("AtkPhantomModo").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            MessageBoxW(
                null_mut(),
                wide("ATK Phantom Modo ya está abierto: búscalo junto al reloj.").as_ptr(),
                wide("ATK Phantom Modo").as_ptr(),
                MB_OK | MB_ICONINFORMATION,
            );
            return;
        }
        TASKBAR_CREATED.get_or_init(|| RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));

        let instance = GetModuleHandleW(null());
        let class = wide("AtkPhantomModo");
        let window_class =
            WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance: instance, lpszClassName: class.as_ptr(), ..zeroed() };
        RegisterClassW(&window_class);
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            return;
        }
        WINDOW.store(window, Ordering::Relaxed);

        let (sender, receiver) = channel();
        let _ = WORKER.set(sender);
        let target = window as isize;
        std::thread::spawn(move || worker(receiver, target));

        tray(NIM_ADD);
        send(Command::Refresh);
        SetTimer(window, TIMER_BATTERY, BATTERY_EVERY_MS, None);

        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// El hilo que habla con el mouse: lo busca cuando hace falta y responde cada pedido.
fn worker(receiver: Receiver<Command>, window: isize) {
    let mut mouse: Option<Phantom> = None;
    for command in receiver {
        if let Command::Battery = command {
            let Some(m) = mouse.as_mut().filter(|m| m.wireless()) else { continue };
            let battery = m.battery();
            if let Ok(mut state) = STATE.lock() {
                match battery {
                    Ok(battery) => state.battery = battery.or(state.battery),
                    Err(Gone) => {
                        mouse = None;
                        state.config = None;
                        state.battery = None;
                        state.wireless = None;
                        state.status = Status::NotFound;
                    }
                }
            }
            // SAFETY: la ventana vive mientras corre el programa; PostMessage no toca nada más.
            unsafe { PostMessageW(window as HWND, WM_RESULT, 0, 0) };
            continue;
        }
        let change = matches!(command, Command::Toggle | Command::Set(_));
        let menu = matches!(command, Command::Menu);
        if mouse.is_none() {
            mouse = Phantom::find();
        }
        let previous = STATE.lock().map(|state| state.previous).unwrap_or(None);
        let (snapshot, status) = match mouse.as_mut().map(|m| run(m, command, previous)) {
            None => (None, Status::NotFound),
            Some(Ok(result)) => result,
            Some(Err(Gone)) => {
                mouse = None;
                (None, Status::NotFound)
            }
        };
        if let Ok(mut state) = STATE.lock() {
            state.status = status;
            let config = snapshot.as_ref().map(|snap| snap.config);
            // También cuando se cambió desde ATK HUB: lo que estaba activo pasa a ser la anterior.
            if let (Some(before), Some(now)) = (state.config, config)
                && before != now
            {
                state.previous = Some(before);
            }
            state.config = config;
            state.battery = snapshot.as_ref().and_then(|snap| snap.battery);
            state.wireless = mouse.as_ref().filter(|_| config.is_some()).map(Phantom::wireless);
            if let Some(snap) = &snapshot
                && let Some(mode) = (snap.config as usize).checked_sub(1).and_then(|i| state.modes.get_mut(i))
            {
                *mode = Some(Mode { rate_hz: snap.rate_hz, dpi: snap.dpi, sensor: snap.sensor });
            }
        }
        // SAFETY: la ventana vive mientras corre el programa; PostMessage no toca nada más.
        unsafe { PostMessageW(window as HWND, if menu { WM_MENU } else { WM_RESULT }, 0, 0) };
        if change {
            CHANGING.store(false, Ordering::Relaxed);
        }
    }
}

/// A la que vuelve el clic estando en `current`: la usada antes, o la 1 o la 2 mientras no la hay.
fn back_from(current: u8, previous: Option<u8>) -> u8 {
    previous.filter(|p| *p != current).unwrap_or(if current == 1 { 2 } else { 1 })
}

fn run(mouse: &mut Phantom, command: Command, previous: Option<u8>) -> Result<(Option<Snapshot>, Status), Gone> {
    let target = match command {
        Command::Refresh | Command::Menu | Command::Battery => None,
        // Ya activa: no se le escribe al mouse lo que ya tiene.
        Command::Set(number) => (mouse.config()? != Some(number)).then_some(number),
        Command::Toggle => mouse.config()?.map(|current| back_from(current, previous)),
    };
    let confirmed = match target {
        Some(number) => mouse.set_config(number)?,
        None => true,
    };
    let snapshot = mouse.snapshot()?;
    let status = if snapshot.is_some() && confirmed { Status::Ok } else { Status::NotConfirmed };
    Ok((snapshot, status))
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: mensajes de la ventana de este programa, en su propio hilo.
    unsafe {
        match message {
            WM_DEVICECHANGE if wparam as u32 == DBT_DEVNODES_CHANGED => {
                // Conectar algo avisa varias veces seguidas: se pregunta una vez, cuando se calma.
                SetTimer(window, TIMER_DEVICES, 1000, None);
                0
            }
            WM_TIMER if wparam == TIMER_BATTERY => {
                send(Command::Battery);
                0
            }
            WM_TIMER if wparam == TIMER_DEVICES => {
                KillTimer(window, TIMER_DEVICES);
                send(Command::Refresh);
                0
            }
            WM_RESULT => {
                tray(NIM_MODIFY);
                warn_low_battery();
                0
            }
            WM_MENU => {
                tray(NIM_MODIFY);
                warn_low_battery();
                show_menu(window);
                0
            }
            WM_TRAY => {
                match lparam as u32 {
                    WM_LBUTTONUP => send(Command::Toggle),
                    WM_RBUTTONUP => send(Command::Menu),
                    _ => {}
                }
                0
            }
            WM_DESTROY => {
                tray(NIM_DELETE);
                PostQuitMessage(0);
                0
            }
            _ if Some(&message) == TASKBAR_CREATED.get() => {
                // El Explorador se reinició: el ícono vuelve a la bandeja.
                tray(NIM_ADD);
                0
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

fn battery_text((level, charging): (u8, bool)) -> String {
    if charging { format!("batería {level}\u{a0}% · cargando") } else { format!("batería {level}\u{a0}%") }
}

/// Por dónde está conectado y su batería: «Inalámbrico · batería 90 %».
fn connection_text(wireless: Option<bool>, battery: Option<(u8, bool)>) -> Option<String> {
    let link = wireless.map(|wireless| if wireless { "Inalámbrico" } else { "Cable" });
    let parts: Vec<String> = [link.map(str::to_owned), battery.map(battery_text)].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// El aviso que toca con esta batería: el umbral más bajo cruzado y aún no avisado.
fn low_battery_notice(battery: Option<(u8, bool)>, warned: Option<u8>) -> (Option<u8>, Option<u8>) {
    match battery {
        // Cargando o recuperada: los avisos vuelven a empezar.
        Some((level, charging)) if charging || level > LOW_BATTERY[0] => (None, None),
        Some((level, _)) => {
            let threshold = LOW_BATTERY.iter().copied().filter(|t| level <= *t).min();
            match threshold {
                Some(t) if warned.is_none_or(|w| t < w) => (Some(t), Some(t)),
                _ => (warned, None),
            }
        }
        None => (warned, None),
    }
}

fn warn_low_battery() {
    let Ok(mut state) = STATE.lock() else { return };
    let (warned, notice) = low_battery_notice(state.battery, state.warned);
    state.warned = warned;
    let level = state.battery.map(|(level, _)| level).unwrap_or(0);
    drop(state);
    match notice {
        Some(5) => notify(
            &format!("Batería casi agotada: {level}\u{a0}%"),
            "Conecta el cable del ATK Phantom ahora: se apagará en poco tiempo.",
        ),
        Some(_) => notify(&format!("Batería baja: {level}\u{a0}%"), "Conecta el cable del ATK Phantom cuando puedas."),
        None => {}
    }
}

/// Una notificación de Windows desde el ícono.
fn notify(title: &str, text: &str) {
    // SAFETY: la estructura se inicializa entera; los textos se copian recortados a su campo.
    unsafe {
        let mut data: NOTIFYICONDATAW = zeroed();
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = WINDOW.load(Ordering::Relaxed);
        data.uID = 1;
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_WARNING;
        let title: Vec<u16> = title.encode_utf16().take(data.szInfoTitle.len() - 1).collect();
        data.szInfoTitle[..title.len()].copy_from_slice(&title);
        let text: Vec<u16> = text.encode_utf16().take(data.szInfo.len() - 1).collect();
        data.szInfo[..text.len()].copy_from_slice(&text);
        Shell_NotifyIconW(NIM_MODIFY, &data);
    }
}

fn mode_text(mode: Mode) -> String {
    let rate = mode.rate_hz.map(|hz| format!("{hz}\u{a0}Hz"));
    let dpi = mode.dpi.map(|(x, y)| if x == y { format!("{x}\u{a0}DPI") } else { format!("{x}×{y}\u{a0}DPI") });
    let sensor = mode.sensor.map(|sensor| {
        match sensor {
            Sensor::Basic => "sensor básico",
            Sensor::Competitive => "sensor competitivo",
            Sensor::Max => "sensor MAX",
        }
        .to_owned()
    });
    [rate, dpi, sensor].into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

/// Pone, actualiza o quita el ícono de la bandeja según lo que se sabe del mouse.
fn tray(action: u32) {
    static CURRENT_ICON: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut());
    let (config, status, battery, wireless, previous) = STATE
        .lock()
        .map(|state| (state.config, state.status, state.battery, state.wireless, state.previous))
        .unwrap_or((None, Status::Searching, None, None, None));
    let (label, color) = match config {
        Some(1) => ('1', (37, 99, 235)),
        Some(2) => ('2', (220, 38, 38)),
        Some(3) => ('3', (22, 163, 74)),
        Some(4) => ('4', (217, 119, 6)),
        _ => ('?', (110, 110, 110)),
    };
    let text = match (config, status) {
        (_, Status::Searching) => "ATK Phantom: buscando el mouse…".to_owned(),
        (None, _) => "ATK Phantom: no lo encuentro. ¿Está conectado?".to_owned(),
        (Some(c), Status::NotConfirmed) => format!("ATK Phantom: configuración {c}. El mouse no confirmó el cambio."),
        (Some(c), _) => {
            let connection = connection_text(wireless, battery).map(|t| format!("\n{t}")).unwrap_or_default();
            let back = back_from(c, previous);
            format!("ATK Phantom: configuración {c}{connection}\nClic: a la {back} · Clic derecho: menú")
        }
    };

    // SAFETY: la estructura se inicializa entera; el ícono anterior se destruye una vez reemplazado.
    unsafe {
        let mut data: NOTIFYICONDATAW = zeroed();
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = WINDOW.load(Ordering::Relaxed);
        data.uID = 1;
        if action == NIM_DELETE {
            Shell_NotifyIconW(NIM_DELETE, &data);
            return;
        }
        let icon: HICON = icon::make(label, color);
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = icon;
        let tip: Vec<u16> = text.encode_utf16().take(data.szTip.len() - 1).collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(action, &data);
        let old = CURRENT_ICON.swap(icon, Ordering::Relaxed);
        if !old.is_null() {
            DestroyIcon(old);
        }
    }
}

fn show_menu(window: HWND) {
    let (active, battery, wireless, modes) = STATE
        .lock()
        .map(|state| (state.config, state.battery, state.wireless, state.modes))
        .unwrap_or((None, None, None, [None; 4]));
    // Lo que se sabe de una configuración va a la derecha (tras el tabulador).
    let configs: Vec<Vec<u16>> = (1..=4u8)
        .map(|config| {
            let label = format!("Configuración {config}");
            let details = modes[config as usize - 1].map(mode_text).filter(|t| !t.is_empty());
            wide(&details.map(|d| format!("{label}\t{d}")).unwrap_or(label))
        })
        .collect();
    // SAFETY: el menú se crea y se destruye aquí; las cadenas viven hasta que se muestra.
    unsafe {
        let menu = CreatePopupMenu();
        let connection = connection_text(wireless, battery).map(|t| wide(&t));
        let hub = wide("Abrir ATK HUB");
        let startup = wide("Iniciar con Windows");
        let exit = wide("Salir");
        if let Some(connection) = &connection {
            AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, connection.as_ptr());
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
        }
        for (config, label) in (1..=4u8).zip(&configs) {
            let checked = if active == Some(config) { MF_CHECKED } else { 0 };
            AppendMenuW(menu, MF_STRING | checked, MENU_CONFIG + config as usize, label.as_ptr());
        }
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, MENU_HUB, hub.as_ptr());
        let starts = if starts_with_windows() { MF_CHECKED } else { 0 };
        AppendMenuW(menu, MF_STRING | starts, MENU_STARTUP, startup.as_ptr());
        AppendMenuW(menu, MF_STRING, MENU_EXIT, exit.as_ptr());

        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        // Sin esto, el menú no se cierra al hacer clic fuera.
        SetForegroundWindow(window);
        let choice = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, 0, window, null());
        PostMessageW(window, WM_NULL, 0, 0);
        DestroyMenu(menu);

        match choice as usize {
            n if (MENU_CONFIG + 1..=MENU_CONFIG + 4).contains(&n) => send(Command::Set((n - MENU_CONFIG) as u8)),
            MENU_HUB => {
                ShellExecuteW(
                    null_mut(),
                    wide("open").as_ptr(),
                    wide("https://hub.atk.pro/").as_ptr(),
                    null(),
                    null(),
                    SW_SHOWNORMAL,
                );
            }
            MENU_STARTUP => toggle_startup(),
            MENU_EXIT => {
                DestroyWindow(window);
            }
            _ => {}
        }
    }
}

fn open_run_key(access: u32) -> Option<HKEY> {
    let mut key: HKEY = null_mut();
    // SAFETY: la clave la cierra quien la pidió.
    let result = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, access, &mut key) };
    (result == ERROR_SUCCESS).then_some(key)
}

fn starts_with_windows() -> bool {
    let Some(key) = open_run_key(KEY_QUERY_VALUE) else { return false };
    // SAFETY: solo pregunta si el valor existe.
    unsafe {
        let found = RegQueryValueExW(key, wide(RUN_NAME).as_ptr(), null(), null_mut(), null_mut(), null_mut());
        RegCloseKey(key);
        found == ERROR_SUCCESS
    }
}

fn toggle_startup() {
    let starts = starts_with_windows();
    let Some(key) = open_run_key(KEY_SET_VALUE) else { return };
    // SAFETY: el valor es una cadena terminada en 0; la clave se cierra al final.
    unsafe {
        if starts {
            RegDeleteValueW(key, wide(RUN_NAME).as_ptr());
        } else {
            let mut path = [0u16; 1024];
            let len = GetModuleFileNameW(null_mut(), path.as_mut_ptr(), path.len() as u32) as usize;
            let exe = String::from_utf16_lossy(&path[..len]);
            let command = wide(&format!("\"{exe}\""));
            RegSetValueExW(
                key,
                wide(RUN_NAME).as_ptr(),
                0,
                REG_SZ,
                command.as_ptr().cast(),
                (command.len() * 2) as u32,
            );
        }
        RegCloseKey(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_click_goes_back_to_the_one_used_before() {
        assert_eq!(back_from(2, Some(1)), 1);
        assert_eq!(back_from(1, Some(3)), 3);
        assert_eq!(back_from(3, Some(2)), 2);
    }

    #[test]
    fn warns_once_below_15_and_again_below_5() {
        let mut warned = None;
        let mut notices = Vec::new();
        for level in [40, 16, 15, 14, 12, 6, 5, 4, 3] {
            let (now, notice) = low_battery_notice(Some((level, false)), warned);
            warned = now;
            notices.extend(notice);
        }
        assert_eq!(notices, [15, 5]);
    }

    #[test]
    fn charging_starts_the_warnings_again() {
        let (warned, _) = low_battery_notice(Some((4, false)), None);
        assert_eq!(low_battery_notice(Some((4, true)), warned), (None, None));
        assert_eq!(low_battery_notice(Some((14, false)), None), (Some(15), Some(15)));
    }

    #[test]
    fn a_first_reading_at_3_warns_only_the_last_one() {
        assert_eq!(low_battery_notice(Some((3, false)), None), (Some(5), Some(5)));
    }

    #[test]
    fn without_one_before_it_alternates_the_first_two() {
        assert_eq!(back_from(1, None), 2);
        assert_eq!(back_from(2, None), 1);
        assert_eq!(back_from(4, None), 1);
        assert_eq!(back_from(2, Some(2)), 1);
    }
}
