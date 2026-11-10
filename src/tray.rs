//! El ícono junto al reloj: ponerlo, cambiarlo, quitarlo y mostrar notificaciones desde él.

use std::mem::{size_of, zeroed};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_RESPECT_QUIET_TIME, NIIF_WARNING, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSMICON;

use crate::icon::{self, Icon, Rgb};
use crate::win::fill;

/// Cómo se ve el ícono.
pub struct Look {
    pub label: String,
    pub color: Rgb,
    pub tooltip: String,
}

pub struct Tray {
    window: HWND,
    /// El mensaje con el que llegan los clics y las teclas sobre el ícono.
    message: u32,
    /// El ícono que muestra la bandeja: vive hasta que lo reemplaza otro.
    icon: Option<Icon>,
    /// Si la bandeja lo tiene (el Explorador lo olvida al reiniciarse).
    added: bool,
}

impl Tray {
    pub fn new(window: HWND, message: u32) -> Tray {
        Tray { window, message, icon: None, added: false }
    }

    pub fn show(&mut self, look: &Look) {
        let mut data = self.data();
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = self.message;
        let icon = icon::badge(&look.label, look.color, self.icon_size());
        data.hIcon = icon.as_ref().map_or(std::ptr::null_mut(), Icon::handle);
        fill(&mut data.szTip, &look.tooltip);
        // SAFETY: `data` está completo y su ícono vive hasta que lo reemplaza el siguiente.
        unsafe {
            if !self.added || Shell_NotifyIconW(NIM_MODIFY, &data) == 0 {
                self.added = Shell_NotifyIconW(NIM_ADD, &data) != 0;
                // La versión 4: clic y teclado como en el resto de Windows (NIN_SELECT,
                // WM_CONTEXTMENU con la posición del ícono).
                data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                Shell_NotifyIconW(NIM_SETVERSION, &data);
            }
        }
        self.icon = icon;
    }

    /// El Explorador se reinició: la bandeja ya no tiene el ícono.
    pub fn forget(&mut self) {
        self.added = false;
    }

    pub fn warn(&self, title: &str, body: &str) {
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_WARNING | NIIF_RESPECT_QUIET_TIME;
        fill(&mut data.szInfoTitle, title);
        fill(&mut data.szInfo, body);
        // SAFETY: `data` está completo.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    pub fn remove(&mut self) {
        // SAFETY: `data` identifica el ícono de esta ventana.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &self.data()) };
        self.added = false;
    }

    fn data(&self) -> NOTIFYICONDATAW {
        // SAFETY: NOTIFYICONDATAW es datos planos; en cero es su estado inicial.
        let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.window;
        data.uID = 1;
        data
    }

    /// El lado de un ícono chico al escalado de la pantalla: 16 px al 100 %, 24 al 150 %…
    fn icon_size(&self) -> i32 {
        // SAFETY: la ventana es de este programa.
        unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(self.window)) }
    }
}
