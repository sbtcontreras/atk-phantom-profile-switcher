//! Windows' theme: dark menus when Windows is dark, as in Explorer's own, and whether the shell
//! (taskbar, flyouts) is light, which the on-screen notice follows.
//!
//! Windows offers dark menus to a classic app only through two functions of `uxtheme.dll`
//! exported by number alone (135 `SetPreferredAppMode` and 136 `FlushMenuThemes`, since Windows 10
//! 1903), the ones many apps use. Without them, menus stay light and nothing else changes.

use std::mem::size_of;
use std::ptr::null_mut;

use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

use crate::win::wide;

const SET_PREFERRED_APP_MODE: usize = 135;
const FLUSH_MENU_THEMES: usize = 136;
/// `PreferredAppMode::AllowDark`: follow Windows' theme.
const ALLOW_DARK: i32 = 1;

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

/// Menus follow Windows' theme from now on.
pub fn follow_system() {
    // SAFETY: the signature is `SetPreferredAppMode(PreferredAppMode) -> PreferredAppMode`.
    if let Some(set_mode) = unsafe { export::<unsafe extern "system" fn(i32) -> i32>(SET_PREFERRED_APP_MODE) } {
        // SAFETY: the uxtheme function with its signature.
        unsafe { set_mode(ALLOW_DARK) };
    }
    refresh();
}

/// Applies a change of Windows' theme to menus.
pub fn refresh() {
    // SAFETY: the signature is `FlushMenuThemes()`.
    if let Some(flush) = unsafe { export::<unsafe extern "system" fn()>(FLUSH_MENU_THEMES) } {
        // SAFETY: the uxtheme function with its signature.
        unsafe { flush() };
    }
}

/// Whether the taskbar and flyouts are light (Settings › Personalization › Colors).
pub fn light_shell() -> bool {
    let mut light = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: reads a DWORD into `light`; a missing value leaves Windows' default, dark.
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(PERSONALIZE).as_ptr(),
            wide("SystemUsesLightTheme").as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&raw mut light).cast(),
            &mut size,
        )
    };
    light != 0
}

/// The function of `uxtheme.dll` exported as number `ordinal`.
///
/// # Safety
/// `F` must be that function's signature.
unsafe fn export<F: Copy>(ordinal: usize) -> Option<F> {
    // SAFETY: uxtheme is already loaded in every process with windows; this only references it.
    let module = unsafe { LoadLibraryW(wide("uxtheme.dll").as_ptr()) };
    if module.is_null() {
        return None;
    }
    // SAFETY: an export number is passed as a pointer with that value (MAKEINTRESOURCE).
    let function = unsafe { GetProcAddress(module, ordinal as *const u8) }?;
    // SAFETY: by contract `F` is the function's signature, a pointer-sized function pointer.
    Some(unsafe { std::mem::transmute_copy(&function) })
}
