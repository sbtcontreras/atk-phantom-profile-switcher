//! El menú del ícono: lo que se sabe del mouse, sus 4 configuraciones y las acciones.

use std::ptr::null;

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, DestroyMenu, HMENU, MF_BYCOMMAND, MF_CHECKED, MF_GRAYED,
    MF_SEPARATOR, MF_STRING, PostMessageW, SetForegroundWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    WM_NULL,
};

use crate::device::Slot;
use crate::strings;
use crate::win::wide;

/// Lo que muestra el menú.
pub struct Model {
    /// «Inalámbrico · batería 90 %», si se sabe.
    pub status: Option<String>,
    /// Cada configuración con lo que se sabe de ella.
    pub slots: [(Slot, String); 4],
    pub active: Option<Slot>,
    pub starts_with_windows: bool,
}

/// Lo que eligió el usuario.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Activate(Slot),
    OpenHub,
    ToggleStartup,
    Exit,
}

const SLOT_BASE: u32 = 100;
const OPEN_HUB: u32 = 200;
const TOGGLE_STARTUP: u32 = 201;
const EXIT: u32 = 202;

/// Muestra el menú en `at` y espera la elección; `None` si se cerró sin elegir.
pub fn show(window: HWND, model: &Model, at: POINT) -> Option<Choice> {
    let menu = Menu::build(model)?;
    // SAFETY: el menú vive hasta el final de esta función; la ventana es de este programa.
    let chosen = unsafe {
        // Sin esto, el menú no se cierra al hacer clic fuera de él.
        SetForegroundWindow(window);
        let chosen = TrackPopupMenu(menu.0, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, 0, window, null());
        PostMessageW(window, WM_NULL, 0, 0);
        chosen as u32
    };
    choice(chosen)
}

fn choice(id: u32) -> Option<Choice> {
    match id {
        OPEN_HUB => Some(Choice::OpenHub),
        TOGGLE_STARTUP => Some(Choice::ToggleStartup),
        EXIT => Some(Choice::Exit),
        id => Slot::new(u8::try_from(id.checked_sub(SLOT_BASE)?).ok()?).map(Choice::Activate),
    }
}

fn slot_id(slot: Slot) -> u32 {
    SLOT_BASE + u32::from(slot.number())
}

/// Un menú, destruido al soltarse.
struct Menu(HMENU);

impl Menu {
    fn build(model: &Model) -> Option<Menu> {
        // SAFETY: crea un menú vacío; queda en `Menu`, que lo destruye.
        let menu = Menu(unsafe { CreatePopupMenu() });
        if menu.0.is_null() {
            return None;
        }
        if let Some(status) = &model.status {
            menu.item(status, 0, MF_GRAYED);
            menu.separator();
        }
        for (slot, details) in &model.slots {
            // Lo que se sabe de cada una va a la derecha, tras el tabulador.
            let label =
                if details.is_empty() { strings::slot(*slot) } else { format!("{}\t{details}", strings::slot(*slot)) };
            menu.item(&label, slot_id(*slot), 0);
        }
        if let Some(active) = model.active {
            let (first, last) = (slot_id(Slot::ALL[0]), slot_id(Slot::ALL[3]));
            // SAFETY: los ítems existen en este menú.
            unsafe { CheckMenuRadioItem(menu.0, first, last, slot_id(active), MF_BYCOMMAND) };
        }
        menu.separator();
        menu.item(strings::OPEN_HUB, OPEN_HUB, 0);
        menu.item(strings::START_WITH_WINDOWS, TOGGLE_STARTUP, if model.starts_with_windows { MF_CHECKED } else { 0 });
        menu.item(strings::EXIT, EXIT, 0);
        Some(menu)
    }

    fn item(&self, text: &str, id: u32, flags: u32) {
        // `&` marca la tecla de acceso en un menú: se escribe doble para mostrarse tal cual.
        let text = wide(&text.replace('&', "&&"));
        // SAFETY: el menú es válido y Windows copia el texto.
        unsafe { AppendMenuW(self.0, MF_STRING | flags, id as usize, text.as_ptr()) };
    }

    fn separator(&self) {
        // SAFETY: el menú es válido.
        unsafe { AppendMenuW(self.0, MF_SEPARATOR, 0, null()) };
    }
}

impl Drop for Menu {
    fn drop(&mut self) {
        // SAFETY: menú creado por `Menu::build`.
        unsafe { DestroyMenu(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_item_maps_back_to_its_choice() {
        for slot in Slot::ALL {
            assert_eq!(choice(slot_id(slot)), Some(Choice::Activate(slot)));
        }
        assert_eq!(choice(OPEN_HUB), Some(Choice::OpenHub));
        assert_eq!(choice(EXIT), Some(Choice::Exit));
        assert_eq!(choice(0), None);
        assert_eq!(choice(SLOT_BASE), None);
    }
}
