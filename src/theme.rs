//! Menús oscuros cuando Windows usa el tema oscuro, como los del propio Explorador.
//!
//! Windows no lo ofrece de forma documentada a una aplicación clásica: se pide con dos funciones
//! de `uxtheme.dll` que solo se exportan por número (135 `SetPreferredAppMode` y 136
//! `FlushMenuThemes`, desde Windows 10 1903), las mismas que usan muchas aplicaciones. Si no
//! están, los menús quedan claros y nada más cambia.

use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::win::wide;

const SET_PREFERRED_APP_MODE: usize = 135;
const FLUSH_MENU_THEMES: usize = 136;
/// `PreferredAppMode::AllowDark`: seguir el tema de Windows.
const ALLOW_DARK: i32 = 1;

/// Los menús siguen el tema de Windows desde ahora.
pub fn follow_system() {
    // SAFETY: la firma es la de `SetPreferredAppMode(PreferredAppMode) -> PreferredAppMode`.
    if let Some(set_mode) = unsafe { export::<unsafe extern "system" fn(i32) -> i32>(SET_PREFERRED_APP_MODE) } {
        // SAFETY: función de uxtheme con su firma.
        unsafe { set_mode(ALLOW_DARK) };
    }
    refresh();
}

/// Aplica a los menús un cambio de tema de Windows.
pub fn refresh() {
    // SAFETY: la firma es la de `FlushMenuThemes()`.
    if let Some(flush) = unsafe { export::<unsafe extern "system" fn()>(FLUSH_MENU_THEMES) } {
        // SAFETY: función de uxtheme con su firma.
        unsafe { flush() };
    }
}

/// La función de `uxtheme.dll` exportada con el número `ordinal`.
///
/// # Safety
/// `F` debe ser la firma de esa función.
unsafe fn export<F: Copy>(ordinal: usize) -> Option<F> {
    // SAFETY: uxtheme ya está cargada en todo proceso con ventanas; LoadLibrary solo la referencia.
    let module = unsafe { LoadLibraryW(wide("uxtheme.dll").as_ptr()) };
    if module.is_null() {
        return None;
    }
    // SAFETY: un número de exportación se pasa como un puntero con ese valor (MAKEINTRESOURCE).
    let function = unsafe { GetProcAddress(module, ordinal as *const u8) }?;
    // SAFETY: según el contrato, `F` es la firma de la función, del mismo tamaño que un puntero.
    Some(unsafe { std::mem::transmute_copy(&function) })
}
