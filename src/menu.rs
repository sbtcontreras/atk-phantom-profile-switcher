//! The tray menu: what is known of the mouse, its 4 profiles (each with its badge, the active
//! one bold and checked) and the actions.

use std::mem::{size_of, zeroed};
use std::ptr::null;

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, DestroyMenu, HMENU, MENUITEMINFOW, MF_BYCOMMAND, MF_CHECKED,
    MF_GRAYED, MF_SEPARATOR, MF_STRING, MIIM_BITMAP, PostMessageW, SetForegroundWindow, SetMenuDefaultItem,
    SetMenuItemInfoW, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL,
};

use crate::badge;
use crate::device::Slot;
use crate::paint::GdiObject;
use crate::strings::Lang;
use crate::win::wide;

/// What the menu shows.
pub struct Model {
    pub lang: Lang,
    /// "Wireless · battery 90%", if known.
    pub status: Option<String>,
    /// Each profile with what is known of it.
    pub slots: [(Slot, String); Slot::ALL.len()],
    pub active: Option<Slot>,
    pub shows_overlay: bool,
    pub starts_with_windows: bool,
    /// The side of a badge, at the screen's scaling.
    pub badge_size: usize,
}

/// What the user picked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Activate(Slot),
    OpenHub,
    ToggleOverlay,
    ToggleStartup,
    Exit,
}

const SLOT_BASE: u32 = 100;
const OPEN_HUB: u32 = 200;
const TOGGLE_OVERLAY: u32 = 201;
const TOGGLE_STARTUP: u32 = 202;
const EXIT: u32 = 203;

/// Shows the menu at `at` and waits for a choice; `None` if it closed without one.
pub fn show(window: HWND, model: &Model, at: POINT) -> Option<Choice> {
    let menu = Menu::build(model)?;
    // SAFETY: the menu lives until the end of this function; the window is this program's.
    let chosen = unsafe {
        // Without this, the menu does not close when clicking outside it.
        SetForegroundWindow(window);
        let chosen = TrackPopupMenu(menu.handle, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, 0, window, null());
        PostMessageW(window, WM_NULL, 0, 0);
        chosen as u32
    };
    choice(chosen)
}

fn choice(id: u32) -> Option<Choice> {
    match id {
        OPEN_HUB => Some(Choice::OpenHub),
        TOGGLE_OVERLAY => Some(Choice::ToggleOverlay),
        TOGGLE_STARTUP => Some(Choice::ToggleStartup),
        EXIT => Some(Choice::Exit),
        id => Slot::new(u8::try_from(id.checked_sub(SLOT_BASE)?).ok()?).map(Choice::Activate),
    }
}

fn slot_id(slot: Slot) -> u32 {
    SLOT_BASE + u32::from(slot.number())
}

/// A menu and the badges it shows, destroyed together when dropped.
struct Menu {
    handle: HMENU,
    badges: Vec<GdiObject>,
}

impl Menu {
    fn build(model: &Model) -> Option<Menu> {
        // SAFETY: creates an empty menu, owned by `Menu`, which destroys it.
        let mut menu = Menu { handle: unsafe { CreatePopupMenu() }, badges: Vec::new() };
        if menu.handle.is_null() {
            return None;
        }
        let lang = model.lang;
        if let Some(status) = &model.status {
            menu.item(status, 0, MF_GRAYED);
            menu.separator();
        }
        for (slot, details) in &model.slots {
            // What is known of each profile goes to the right, after the tab.
            let name = lang.profile_name(*slot);
            let label = if details.is_empty() { name } else { format!("{name}\t{details}") };
            menu.item(&label, slot_id(*slot), 0);
            if let Some(bitmap) = badge::draw(Some(*slot), model.badge_size).bitmap() {
                menu.picture(slot_id(*slot), bitmap);
            }
        }
        if let Some(active) = model.active {
            let [first, .., last] = Slot::ALL;
            // SAFETY: the items exist in this menu.
            unsafe {
                CheckMenuRadioItem(menu.handle, slot_id(first), slot_id(last), slot_id(active), MF_BYCOMMAND);
                SetMenuDefaultItem(menu.handle, slot_id(active), 0);
            }
        }
        menu.separator();
        menu.item(lang.open_hub(), OPEN_HUB, 0);
        menu.item(lang.show_overlay(), TOGGLE_OVERLAY, if model.shows_overlay { MF_CHECKED } else { 0 });
        menu.item(lang.start_with_windows(), TOGGLE_STARTUP, if model.starts_with_windows { MF_CHECKED } else { 0 });
        menu.item(lang.exit(), EXIT, 0);
        Some(menu)
    }

    fn item(&self, text: &str, id: u32, flags: u32) {
        // `&` marks a menu's access key: doubled, it shows as itself.
        let text = wide(&text.replace('&', "&&"));
        // SAFETY: a valid menu; Windows copies the text.
        unsafe { AppendMenuW(self.handle, MF_STRING | flags, id as usize, text.as_ptr()) };
    }

    /// Puts `bitmap` beside item `id`; the menu keeps it until it is destroyed.
    fn picture(&mut self, id: u32, bitmap: GdiObject) {
        // SAFETY: MENUITEMINFOW is plain data; only the bitmap is set.
        unsafe {
            let mut info: MENUITEMINFOW = zeroed();
            info.cbSize = size_of::<MENUITEMINFOW>() as u32;
            info.fMask = MIIM_BITMAP;
            info.hbmpItem = bitmap.handle();
            SetMenuItemInfoW(self.handle, id, 0, &info);
        }
        self.badges.push(bitmap);
    }

    fn separator(&self) {
        // SAFETY: a valid menu.
        unsafe { AppendMenuW(self.handle, MF_SEPARATOR, 0, null()) };
    }
}

impl Drop for Menu {
    fn drop(&mut self) {
        // SAFETY: a menu created by `Menu::build`; its badges are deleted after it.
        unsafe { DestroyMenu(self.handle) };
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
        assert_eq!(choice(TOGGLE_OVERLAY), Some(Choice::ToggleOverlay));
        assert_eq!(choice(TOGGLE_STARTUP), Some(Choice::ToggleStartup));
        assert_eq!(choice(EXIT), Some(Choice::Exit));
        assert_eq!(choice(0), None);
        assert_eq!(choice(SLOT_BASE), None);
    }
}
