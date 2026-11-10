//! Every text of the interface, in Spanish when Windows shows Spanish and in English otherwise.

use std::sync::OnceLock;

use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;

use crate::device::{Battery, Connection, Profile, Sensor, Slot};

pub const APP_NAME: &str = "Phantom Switch";

/// A non-breaking space, between a number and its unit.
const NBSP: char = '\u{a0}';
/// `LANG_SPANISH`, the primary language of every Spanish locale.
const SPANISH: u16 = 0x0A;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Spanish,
    English,
}

/// The language of the interface: Windows' display language, read once.
pub fn ui() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(|| {
        // SAFETY: no arguments; returns the user's display language.
        let language = unsafe { GetUserDefaultUILanguage() };
        if language & 0x3FF == SPANISH { Lang::Spanish } else { Lang::English }
    })
}

impl Lang {
    fn pick(self, spanish: &'static str, english: &'static str) -> &'static str {
        match self {
            Lang::Spanish => spanish,
            Lang::English => english,
        }
    }

    pub fn already_running(self) -> &'static str {
        self.pick(
            "Phantom Switch ya está abierto: búscalo junto al reloj.",
            "Phantom Switch is already running: look for it next to the clock.",
        )
    }

    pub fn searching(self) -> &'static str {
        self.pick("Buscando el mouse…", "Looking for the mouse…")
    }

    pub fn not_found(self) -> &'static str {
        self.pick("No encuentro el ATK Phantom. ¿Está conectado?", "Can't find the ATK Phantom. Is it connected?")
    }

    pub fn no_answer(self) -> &'static str {
        self.pick("El mouse no responde. ¿Está encendido?", "The mouse isn't answering. Is it on?")
    }

    pub fn open_hub(self) -> &'static str {
        self.pick("Abrir ATK HUB", "Open ATK HUB")
    }

    pub fn show_overlay(self) -> &'static str {
        self.pick("Mostrar aviso al cambiar", "Show a notice when switching")
    }

    pub fn start_with_windows(self) -> &'static str {
        self.pick("Iniciar con Windows", "Start with Windows")
    }

    pub fn exit(self) -> &'static str {
        self.pick("Salir", "Exit")
    }

    pub fn switch_failed(self) -> &'static str {
        self.pick("No se pudo cambiar de configuración", "Couldn't switch profiles")
    }

    pub fn switch_unconfirmed(self) -> &'static str {
        self.pick(
            "El mouse no confirmó el cambio. Vuelve a intentarlo.",
            "The mouse didn't confirm the switch. Try again.",
        )
    }

    pub fn slot(self, slot: Slot) -> String {
        match self {
            Lang::Spanish => format!("Configuración {}", slot.number()),
            Lang::English => format!("Profile {}", slot.number()),
        }
    }

    pub fn click_hint(self, target: Slot) -> String {
        match self {
            Lang::Spanish => format!("Clic: cambiar a la {}", target.number()),
            Lang::English => format!("Click: switch to profile {}", target.number()),
        }
    }

    fn percent(self, value: u8) -> String {
        match self {
            Lang::Spanish => format!("{value}{NBSP}%"),
            Lang::English => format!("{value}%"),
        }
    }

    /// "Wireless · battery 90%", or as much of it as is known.
    pub fn connection(self, connection: Option<Connection>, battery: Option<Battery>) -> Option<String> {
        let link = connection.map(|c| match c {
            Connection::Wireless => self.pick("Inalámbrico", "Wireless").to_owned(),
            Connection::Wired => self.pick("Cable", "Wired").to_owned(),
        });
        let battery = battery.map(|b| {
            let level = format!("{} {}", self.pick("batería", "battery"), self.percent(b.percent));
            if b.charging { format!("{level} · {}", self.pick("cargando", "charging")) } else { level }
        });
        let parts: Vec<String> = [link, battery].into_iter().flatten().collect();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    /// "4000 Hz · 1600 DPI · competitive sensor", or as much of it as is known.
    pub fn profile(self, profile: &Profile) -> String {
        let polling = profile.polling_hz.map(|hz| format!("{hz}{NBSP}Hz"));
        let dpi = profile.dpi.map(|(x, y)| if x == y { format!("{x}{NBSP}DPI") } else { format!("{x}×{y}{NBSP}DPI") });
        let sensor = profile.sensor.map(|sensor| {
            match sensor {
                Sensor::Basic => self.pick("sensor básico", "basic sensor"),
                Sensor::Competitive => self.pick("sensor competitivo", "competitive sensor"),
                Sensor::Max => self.pick("sensor MAX", "MAX sensor"),
            }
            .to_owned()
        });
        [polling, dpi, sensor].into_iter().flatten().collect::<Vec<_>>().join(" · ")
    }

    /// Title and body of the low-battery notice for crossing `threshold`.
    pub fn low_battery(self, threshold: u8, percent: u8) -> (String, &'static str) {
        let level = self.percent(percent);
        if threshold <= 5 {
            (
                format!("{} {level}", self.pick("Batería casi agotada:", "Battery almost empty:")),
                self.pick(
                    "Conecta el cable del ATK Phantom ahora: se apagará pronto.",
                    "Plug in the ATK Phantom's cable now: it will turn off soon.",
                ),
            )
        } else {
            (
                format!("{} {level}", self.pick("Batería baja:", "Low battery:")),
                self.pick(
                    "Conecta el cable del ATK Phantom cuando puedas.",
                    "Plug in the ATK Phantom's cable when you can.",
                ),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIRELESS_90: Option<Battery> = Some(Battery { percent: 90, charging: false });
    const CHARGING_40: Option<Battery> = Some(Battery { percent: 40, charging: true });

    #[test]
    fn says_what_it_knows_of_the_connection() {
        let spanish = Lang::Spanish.connection(Some(Connection::Wireless), WIRELESS_90);
        assert_eq!(spanish.as_deref(), Some("Inalámbrico · batería 90\u{a0}%"));
        let english = Lang::English.connection(Some(Connection::Wired), CHARGING_40);
        assert_eq!(english.as_deref(), Some("Wired · battery 40% · charging"));
        assert_eq!(Lang::English.connection(None, None), None);
    }

    #[test]
    fn says_what_it_knows_of_a_profile() {
        let full = Profile { polling_hz: Some(4000), dpi: Some((1600, 1600)), sensor: Some(Sensor::Competitive) };
        assert_eq!(Lang::Spanish.profile(&full), "4000\u{a0}Hz · 1600\u{a0}DPI · sensor competitivo");
        assert_eq!(Lang::English.profile(&full), "4000\u{a0}Hz · 1600\u{a0}DPI · competitive sensor");
        let split = Profile { polling_hz: None, dpi: Some((800, 1600)), sensor: None };
        assert_eq!(Lang::English.profile(&split), "800×1600\u{a0}DPI");
    }

    #[test]
    fn names_profiles_in_each_language() {
        let second = Slot::ALL[1];
        assert_eq!(Lang::Spanish.slot(second), "Configuración 2");
        assert_eq!(Lang::English.slot(second), "Profile 2");
        assert_eq!(Lang::English.low_battery(5, 4).0, "Battery almost empty: 4%");
    }
}
