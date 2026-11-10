//! Todos los textos de la interfaz, en un solo lugar.

use crate::device::{Battery, Connection, Profile, Sensor, Slot};

pub const APP_NAME: &str = "Phantom Switch";
pub const ALREADY_RUNNING: &str = "Phantom Switch ya está abierto: búscalo junto al reloj.";

pub const SEARCHING: &str = "Buscando el mouse…";
pub const NOT_FOUND: &str = "No encuentro el ATK Phantom. ¿Está conectado?";
pub const NO_ANSWER: &str = "El mouse no responde. ¿Está encendido?";

pub const OPEN_HUB: &str = "Abrir ATK HUB";
pub const START_WITH_WINDOWS: &str = "Iniciar con Windows";
pub const EXIT: &str = "Salir";

pub const SWITCH_FAILED: &str = "No se pudo cambiar de configuración";
pub const SWITCH_UNCONFIRMED: &str = "El mouse no confirmó el cambio. Vuelve a intentarlo.";

/// Espacio duro entre un número y su unidad.
const NBSP: char = '\u{a0}';

pub fn slot(slot: Slot) -> String {
    format!("Configuración {}", slot.number())
}

pub fn click_hint(target: Slot) -> String {
    format!("Clic: cambiar a la {}", target.number())
}

/// «Inalámbrico · batería 90 %», o lo que se sepa de eso.
pub fn connection(connection: Option<Connection>, battery: Option<Battery>) -> Option<String> {
    let link = connection.map(|c| match c {
        Connection::Wireless => "Inalámbrico".to_owned(),
        Connection::Wired => "Cable".to_owned(),
    });
    let battery = battery.map(|b| {
        let level = format!("batería {}{NBSP}%", b.percent);
        if b.charging { format!("{level} · cargando") } else { level }
    });
    let parts: Vec<String> = [link, battery].into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// «4000 Hz · 1600 DPI · sensor competitivo», o lo que se sepa de eso.
pub fn profile(profile: &Profile) -> String {
    let polling = profile.polling_hz.map(|hz| format!("{hz}{NBSP}Hz"));
    let dpi = profile.dpi.map(|(x, y)| if x == y { format!("{x}{NBSP}DPI") } else { format!("{x}×{y}{NBSP}DPI") });
    let sensor = profile.sensor.map(|sensor| {
        match sensor {
            Sensor::Basic => "sensor básico",
            Sensor::Competitive => "sensor competitivo",
            Sensor::Max => "sensor MAX",
        }
        .to_owned()
    });
    [polling, dpi, sensor].into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

/// Título y texto del aviso de batería baja al cruzar `threshold`.
pub fn low_battery(threshold: u8, percent: u8) -> (String, &'static str) {
    if threshold <= 5 {
        (
            format!("Batería casi agotada: {percent}{NBSP}%"),
            "Conecta el cable del ATK Phantom ahora: se apagará pronto.",
        )
    } else {
        (format!("Batería baja: {percent}{NBSP}%"), "Conecta el cable del ATK Phantom cuando puedas.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_what_it_knows_of_the_connection() {
        let battery = Battery { percent: 90, charging: false };
        assert_eq!(
            connection(Some(Connection::Wireless), Some(battery)).as_deref(),
            Some("Inalámbrico · batería 90\u{a0}%")
        );
        let charging = Battery { percent: 40, charging: true };
        assert_eq!(
            connection(Some(Connection::Wired), Some(charging)).as_deref(),
            Some("Cable · batería 40\u{a0}% · cargando")
        );
        assert_eq!(connection(None, None), None);
    }

    #[test]
    fn says_what_it_knows_of_a_profile() {
        let full = Profile { polling_hz: Some(4000), dpi: Some((1600, 1600)), sensor: Some(Sensor::Competitive) };
        assert_eq!(profile(&full), "4000\u{a0}Hz · 1600\u{a0}DPI · sensor competitivo");
        let split = Profile { polling_hz: None, dpi: Some((800, 1600)), sensor: None };
        assert_eq!(profile(&split), "800×1600\u{a0}DPI");
    }
}
