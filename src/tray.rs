//! The icon next to the clock: adding, changing and removing it, and notifications from it.

use std::mem::{size_of, zeroed};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_RESPECT_QUIET_TIME, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SM_CXSMICON;

use crate::badge;
use crate::device::Slot;
use crate::paint::Icon;
use crate::win::fill;

pub struct Tray {
    window: HWND,
    /// The message that brings clicks and keys on the icon.
    message: u32,
    /// The icon the tray shows: it lives until another replaces it.
    icon: Option<Icon>,
    /// Whether the tray has it (Explorer forgets it when it restarts).
    added: bool,
}

impl Tray {
    pub fn new(window: HWND, message: u32) -> Tray {
        Tray { window, message, icon: None, added: false }
    }

    /// Shows the badge of `slot` (a question mark when unknown) with `tooltip`.
    pub fn show(&mut self, slot: Option<Slot>, tooltip: &str) {
        let mut data = self.data();
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = self.message;
        let icon = badge::draw(slot, self.icon_size()).icon();
        data.hIcon = icon.as_ref().map_or(std::ptr::null_mut(), Icon::handle);
        fill(&mut data.szTip, tooltip);
        // SAFETY: `data` is complete, and its icon lives until the next one replaces it.
        unsafe {
            if !self.added || Shell_NotifyIconW(NIM_MODIFY, &data) == 0 {
                self.added = Shell_NotifyIconW(NIM_ADD, &data) != 0;
                // Version 4: clicks and keys as everywhere else in Windows (NIN_SELECT, and
                // WM_CONTEXTMENU with the icon's position).
                data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                Shell_NotifyIconW(NIM_SETVERSION, &data);
            }
        }
        self.icon = icon;
    }

    /// Explorer restarted: the tray no longer has the icon.
    pub fn forget(&mut self) {
        self.added = false;
    }

    pub fn warn(&self, title: &str, body: &str) {
        self.notify(NIIF_WARNING, title, body);
    }

    pub fn tell(&self, title: &str, body: &str) {
        self.notify(NIIF_INFO, title, body);
    }

    fn notify(&self, kind: u32, title: &str, body: &str) {
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = kind | NIIF_RESPECT_QUIET_TIME;
        fill(&mut data.szInfoTitle, title);
        fill(&mut data.szInfo, body);
        // SAFETY: `data` is complete.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    pub fn remove(&mut self) {
        // SAFETY: `data` names this window's icon.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &self.data()) };
        self.added = false;
    }

    /// The side of a small icon at the screen's scaling: 16 px at 100%, 24 at 150%…
    pub fn icon_size(&self) -> usize {
        // SAFETY: the window is this program's.
        let side = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(self.window)) };
        usize::try_from(side).unwrap_or(16).clamp(16, 256)
    }

    fn data(&self) -> NOTIFYICONDATAW {
        // SAFETY: NOTIFYICONDATAW is plain data; all zeros is its initial state.
        let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.window;
        data.uID = 1;
        data
    }
}
