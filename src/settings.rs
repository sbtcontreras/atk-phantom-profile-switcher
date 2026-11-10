//! What the user chooses, kept in their registry (no admin rights, no files beside the `.exe`):
//! whether the app starts with Windows (`HKCU\…\Run`) and whether it shows the on-screen notice
//! (`HKCU\Software\PhantomSwitch`).

use std::mem::size_of;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
    RRF_RT_REG_DWORD, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW,
};

use crate::win::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "PhantomSwitch";
const APP_KEY: &str = r"Software\PhantomSwitch";
const SHOW_OVERLAY: &str = "ShowOverlay";

pub fn starts_with_windows() -> bool {
    let Some(key) = Key::open(RUN_KEY, KEY_QUERY_VALUE) else { return false };
    // SAFETY: only asks whether the value exists.
    let found =
        unsafe { RegQueryValueExW(key.0, wide(RUN_VALUE).as_ptr(), null(), null_mut(), null_mut(), null_mut()) };
    found == ERROR_SUCCESS
}

pub fn set_starts_with_windows(enabled: bool) {
    let Some(key) = Key::open(RUN_KEY, KEY_SET_VALUE) else { return };
    if !enabled {
        // SAFETY: a NUL-terminated name.
        unsafe { RegDeleteValueW(key.0, wide(RUN_VALUE).as_ptr()) };
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let command = wide(&format!("\"{}\"", exe.display()));
    // SAFETY: a NUL-terminated string, with its length in bytes.
    unsafe {
        RegSetValueExW(key.0, wide(RUN_VALUE).as_ptr(), 0, REG_SZ, command.as_ptr().cast(), (command.len() * 2) as u32);
    }
}

/// Whether to show the on-screen notice when switching; on until the user turns it off.
pub fn shows_overlay() -> bool {
    let mut value = 1u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: reads a DWORD into `value`; a missing value leaves the default.
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(APP_KEY).as_ptr(),
            wide(SHOW_OVERLAY).as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&raw mut value).cast(),
            &mut size,
        )
    };
    value != 0
}

pub fn set_shows_overlay(enabled: bool) {
    let mut key: HKEY = null_mut();
    // SAFETY: the key is created or opened into `key` and closed by `Key`.
    let created = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(APP_KEY).as_ptr(),
            0,
            null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null(),
            &mut key,
            null_mut(),
        )
    };
    if created != ERROR_SUCCESS {
        return;
    }
    let key = Key(key);
    let value = u32::from(enabled);
    // SAFETY: a DWORD, with its size.
    unsafe {
        RegSetValueExW(
            key.0,
            wide(SHOW_OVERLAY).as_ptr(),
            0,
            REG_DWORD,
            (&raw const value).cast(),
            size_of::<u32>() as u32,
        )
    };
}

/// A registry key of the user, closed when dropped.
struct Key(HKEY);

impl Key {
    fn open(path: &str, access: u32) -> Option<Key> {
        let mut key: HKEY = null_mut();
        // SAFETY: the opened key goes into `Key`, which closes it.
        let result = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(path).as_ptr(), 0, access, &mut key) };
        (result == ERROR_SUCCESS).then_some(Key(key))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: a key opened or created here.
        unsafe { RegCloseKey(self.0) };
    }
}
