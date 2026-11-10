//! The current user's registry (`HKEY_CURRENT_USER`): reading and writing values, no admin rights
//! needed.

use std::mem::size_of;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE,
    RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegSetValueExW,
};

use crate::win::wide;

/// The DWORD `name` under `path`, if it is there.
pub fn dword(path: &str, name: &str) -> Option<u32> {
    let mut value = 0u32;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: NUL-terminated strings; reads a DWORD into `value`, with its size.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(path).as_ptr(),
            wide(name).as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&raw mut value).cast(),
            &mut size,
        )
    };
    (read == ERROR_SUCCESS).then_some(value)
}

/// The string `name` under `path`, if it is there.
pub fn string(path: &str, name: &str) -> Option<String> {
    let (path, name) = (wide(path), wide(name));
    let mut size = 0u32;
    // SAFETY: NUL-terminated strings; without a buffer, Windows only says the size in bytes.
    let sized = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, path.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ, null_mut(), null_mut(), &mut size)
    };
    if sized != ERROR_SUCCESS {
        return None;
    }
    let mut text = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY: `text` has room for `size` bytes; Windows ends the string with a NUL.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            path.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            text.as_mut_ptr().cast(),
            &mut size,
        )
    };
    let len = text.iter().position(|unit| *unit == 0).unwrap_or(text.len());
    (read == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&text[..len]))
}

pub fn set_dword(path: &str, name: &str, value: u32) {
    set(path, name, REG_DWORD, &value.to_le_bytes());
}

pub fn set_string(path: &str, name: &str, value: &str) {
    let bytes: Vec<u8> = wide(value).iter().flat_map(|unit| unit.to_le_bytes()).collect();
    set(path, name, REG_SZ, &bytes);
}

pub fn delete(path: &str, name: &str) {
    // SAFETY: NUL-terminated strings.
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(path).as_ptr(), wide(name).as_ptr()) };
}

fn set(path: &str, name: &str, kind: REG_VALUE_TYPE, bytes: &[u8]) {
    let Some(key) = Key::create(path) else { return };
    // SAFETY: an open key, a NUL-terminated name and the value's bytes with their length.
    unsafe { RegSetValueExW(key.0, wide(name).as_ptr(), 0, kind, bytes.as_ptr(), bytes.len() as u32) };
}

/// A key opened to write to it, created if it is not there, and closed when dropped.
struct Key(HKEY);

impl Key {
    fn create(path: &str) -> Option<Key> {
        let mut key: HKEY = null_mut();
        // SAFETY: the created or opened key goes into `Key`, which closes it.
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                null(),
                &mut key,
                null_mut(),
            )
        };
        (result == ERROR_SUCCESS).then_some(Key(key))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: a key opened by `Key::create`.
        unsafe { RegCloseKey(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::Registry::RegDeleteKeyW;

    use super::*;

    /// A key of its own, deleted at the end.
    const KEY: &str = r"Software\PhantomSwitch\Tests";

    #[test]
    fn values_read_back_as_written_and_go_when_deleted() {
        set_string(KEY, "Text", r#""C:\Apps\Phantom Switch\PhantomSwitch.exe""#);
        set_dword(KEY, "Number", 7);
        assert_eq!(string(KEY, "Text").as_deref(), Some(r#""C:\Apps\Phantom Switch\PhantomSwitch.exe""#));
        assert_eq!(dword(KEY, "Number"), Some(7));
        assert_eq!(string(KEY, "Number"), None);
        delete(KEY, "Text");
        delete(KEY, "Number");
        assert_eq!(string(KEY, "Text"), None);
        // SAFETY: a NUL-terminated path of an empty key this test made.
        unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, wide(KEY).as_ptr()) };
    }
}
