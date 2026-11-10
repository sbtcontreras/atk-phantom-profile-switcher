//! What any part that talks to Windows needs.

use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW, SW_SHOWNORMAL};

use crate::APP_NAME;

/// `text` as NUL-terminated UTF-16.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// Copies `text` into a fixed-length Windows text field, cut short and NUL-terminated.
pub fn fill(field: &mut [u16], text: &str) {
    let room = field.len().saturating_sub(1);
    let units: Vec<u16> = text.encode_utf16().take(room).collect();
    field[..units.len()].copy_from_slice(&units);
    field[units.len()..].fill(0);
}

/// One running copy at a time: while it exists, another copy cannot get it.
pub struct SingleInstance(#[expect(dead_code, reason = "kept only to hold the mutex")] OwnedHandle);

impl SingleInstance {
    pub fn acquire(name: &str) -> Option<SingleInstance> {
        // SAFETY: creates (or opens) a named mutex; the handle goes into an `OwnedHandle`.
        let (handle, existed) = unsafe {
            let handle = CreateMutexW(null(), 1, wide(name).as_ptr());
            (handle, GetLastError() == ERROR_ALREADY_EXISTS)
        };
        if handle.is_null() {
            return None;
        }
        // SAFETY: a valid handle just created, owned by nobody else.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        (!existed).then_some(SingleInstance(handle))
    }
}

pub fn message(text: &str) {
    // SAFETY: NUL-terminated strings that live through the call.
    unsafe {
        MessageBoxW(null_mut(), wide(text).as_ptr(), wide(APP_NAME).as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

/// Opens `url` in the default browser.
pub fn open_url(url: &str) {
    // SAFETY: NUL-terminated strings that live through the call.
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
