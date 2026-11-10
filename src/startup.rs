//! «Iniciar con Windows»: la entrada del programa en el inicio de sesión del usuario
//! (`HKCU\…\Run`), sin permisos de administrador.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};

use crate::win::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "PhantomSwitch";

pub fn is_enabled() -> bool {
    let Some(key) = Key::open(KEY_QUERY_VALUE) else { return false };
    // SAFETY: solo pregunta si el valor existe.
    let found = unsafe { RegQueryValueExW(key.0, wide(VALUE).as_ptr(), null(), null_mut(), null_mut(), null_mut()) };
    found == ERROR_SUCCESS
}

pub fn set_enabled(enabled: bool) {
    let Some(key) = Key::open(KEY_SET_VALUE) else { return };
    if !enabled {
        // SAFETY: nombre terminado en 0.
        unsafe { RegDeleteValueW(key.0, wide(VALUE).as_ptr()) };
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let command = wide(&format!("\"{}\"", exe.display()));
    // SAFETY: el valor es una cadena terminada en 0, con su largo en bytes.
    unsafe {
        RegSetValueExW(key.0, wide(VALUE).as_ptr(), 0, REG_SZ, command.as_ptr().cast(), (command.len() * 2) as u32);
    }
}

/// La clave `Run` del usuario, cerrada al soltarse.
struct Key(HKEY);

impl Key {
    fn open(access: u32) -> Option<Key> {
        let mut key: HKEY = null_mut();
        // SAFETY: la clave abierta queda en `Key`, que la cierra.
        let result = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, access, &mut key) };
        (result == ERROR_SUCCESS).then_some(Key(key))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: clave abierta por `Key::open`.
        unsafe { RegCloseKey(self.0) };
    }
}
