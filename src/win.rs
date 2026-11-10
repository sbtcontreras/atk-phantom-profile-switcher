//! Lo que pide cualquier parte que habla con Windows.

use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW, SW_SHOWNORMAL};

use crate::strings;

/// `text` en UTF-16 terminado en 0.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Copia `text` en un campo de texto de largo fijo de Windows, recortado y terminado en 0.
pub fn fill(field: &mut [u16], text: &str) {
    let room = field.len().saturating_sub(1);
    let units: Vec<u16> = text.encode_utf16().take(room).collect();
    field[..units.len()].copy_from_slice(&units);
    field[units.len()..].fill(0);
}

/// Una sola copia abierta a la vez: mientras exista, otra copia no la consigue.
pub struct SingleInstance(#[expect(dead_code, reason = "se guarda solo para mantener el mutex")] OwnedHandle);

impl SingleInstance {
    pub fn acquire(name: &str) -> Option<SingleInstance> {
        // SAFETY: crea (o abre) un mutex con nombre; el handle queda en un `OwnedHandle`.
        let (handle, existed) = unsafe {
            let handle = CreateMutexW(null(), 1, wide(name).as_ptr());
            (handle, GetLastError() == ERROR_ALREADY_EXISTS)
        };
        if handle.is_null() {
            return None;
        }
        // SAFETY: un handle válido recién creado y de nadie más.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        (!existed).then_some(SingleInstance(handle))
    }
}

pub fn message(text: &str) {
    // SAFETY: cadenas terminadas en 0 que viven durante la llamada.
    unsafe {
        MessageBoxW(null_mut(), wide(text).as_ptr(), wide(strings::APP_NAME).as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

/// Abre `url` en el navegador predeterminado.
pub fn open_url(url: &str) {
    // SAFETY: cadenas terminadas en 0 que viven durante la llamada.
    unsafe {
        ShellExecuteW(null_mut(), wide("open").as_ptr(), wide(url).as_ptr(), null(), null(), SW_SHOWNORMAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_a_field_cut_and_terminated() {
        let mut field = [7u16; 5];
        fill(&mut field, "abcdefg");
        assert_eq!(field, [97, 98, 99, 100, 0]);
        fill(&mut field, "x");
        assert_eq!(field, [120, 0, 0, 0, 0]);
    }
}
