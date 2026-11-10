//! El ATK Phantom Ultimate: encontrarlo, leer lo que tiene y cambiar de configuración. Todo
//! lo que le pide es lectura, salvo `Phantom::activate`.

mod hid;
mod protocol;

use std::time::Duration;

use protocol::{FEATURE_FPS_13K, REPORT_ID, Request, Route};
pub use protocol::{Sensor, Slot};

/// Fabricantes de las interfaces de ATK.
const VENDORS: [u16; 2] = [0x373B, 0x3554];
/// La interfaz de órdenes de los mouse BITMOUSE: página de uso y uso.
const INTERFACE: (u16, u16) = (0xFF05, 1);
/// El Phantom por cable; cualquier otro producto con esa interfaz es un receptor.
const WIRED: [u16; 2] = [4814, 4882];
/// Lo que se espera una respuesta (el receptor tarda más que el cable).
const ANSWER_WAIT: Duration = Duration::from_millis(1500);

/// El mouse dejó de responder a la conexión: se desconectó o se apagó el receptor.
#[derive(Debug)]
pub struct Disconnected;

pub type Result<T> = std::result::Result<T, Disconnected>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Connection {
    Wired,
    Wireless,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

/// Lo que tiene una configuración.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Profile {
    pub polling_hz: Option<u32>,
    /// DPI horizontal y vertical del nivel activo.
    pub dpi: Option<(u16, u16)>,
    pub sensor: Option<Sensor>,
}

/// Lo que se lee del mouse en un momento.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snapshot {
    pub slot: Slot,
    pub profile: Profile,
    pub battery: Option<Battery>,
}

pub struct Phantom {
    link: hid::Link,
    route: Route,
    /// Si el modo del sensor está en la memoria de la configuración (según las funciones del
    /// mouse, preguntadas una vez por conexión).
    sensor_in_memory: Option<bool>,
}

impl Phantom {
    /// La primera interfaz ATK que responde cuál es su configuración activa.
    pub fn find() -> Option<Phantom> {
        hid::interfaces()
            .into_iter()
            .filter(|i| VENDORS.contains(&i.vendor) && (i.usage_page, i.usage) == INTERFACE)
            .filter(|i| i.output_len > protocol::PACKET_LEN)
            .find_map(|interface| {
                let route = if WIRED.contains(&interface.product) { Route::Wired } else { Route::Receiver };
                let mut phantom = Phantom { link: hid::Link::open(&interface)?, route, sensor_in_memory: None };
                matches!(phantom.active_slot(), Ok(Some(_))).then_some(phantom)
            })
    }

    pub fn connection(&self) -> Connection {
        match self.route {
            Route::Wired => Connection::Wired,
            Route::Receiver => Connection::Wireless,
        }
    }

    /// La configuración activa, o `None` si el mouse no contestó.
    pub fn active_slot(&mut self) -> Result<Option<Slot>> {
        Ok(self.ask(Request::Active)?.and_then(|data| protocol::active(&data)).map(|(slot, _)| slot))
    }

    /// Activa `slot`; `false` si el mouse no lo confirmó.
    pub fn activate(&mut self, slot: Slot) -> Result<bool> {
        Ok(self.ask(Request::Activate(slot))?.is_some())
    }

    /// La configuración activa con lo que tiene, y la batería; `None` si el mouse no contestó.
    pub fn snapshot(&mut self) -> Result<Option<Snapshot>> {
        let Some((slot, polling_hz)) = self.ask(Request::Active)?.and_then(|data| protocol::active(&data)) else {
            return Ok(None);
        };
        let profile = Profile { polling_hz, dpi: self.dpi()?, sensor: self.sensor()? };
        Ok(Some(Snapshot { slot, profile, battery: self.battery()? }))
    }

    /// La batería, o `None` si el mouse no contestó.
    pub fn battery(&mut self) -> Result<Option<Battery>> {
        let Some(percent) = self.ask(Request::Battery)?.and_then(|data| data.first().copied()) else {
            return Ok(None);
        };
        let charging = self.ask(Request::Charging)?.is_some_and(|data| data.first() == Some(&1));
        Ok(Some(Battery { percent, charging }))
    }

    /// El DPI del nivel activo: el nivel en la tabla, y luego solo su entrada.
    fn dpi(&mut self) -> Result<Option<(u16, u16)>> {
        let Some(level) = self.read_memory(protocol::DPI_TABLE, 1)? else { return Ok(None) };
        Ok(self.read_memory(protocol::dpi_entry(level[0]), 4)?.and_then(|entry| protocol::dpi(&entry)))
    }

    /// El modo del sensor; `None` si no se sabe leerlo en este mouse.
    fn sensor(&mut self) -> Result<Option<Sensor>> {
        if self.sensor_in_memory.is_none() {
            let features = self.ask(Request::Features)?.and_then(|data| data.first().copied());
            self.sensor_in_memory = features.map(|bits| bits & FEATURE_FPS_13K == 0);
        }
        if self.sensor_in_memory != Some(true) {
            return Ok(None);
        }
        Ok(self.read_memory(protocol::SENSOR_MODE, 1)?.and_then(|mode| protocol::sensor(mode[0])))
    }

    fn read_memory(&mut self, address: u16, len: u8) -> Result<Option<Vec<u8>>> {
        let data = self.ask(Request::Memory { address, len })?;
        Ok(data.and_then(|data| protocol::memory(&data, len).map(<[u8]>::to_vec)))
    }

    /// Hace un pedido y devuelve los datos de su respuesta.
    fn ask(&mut self, request: Request) -> Result<Option<Vec<u8>>> {
        let packet = request.packet(self.route);
        let report = self.link.exchange(REPORT_ID, &packet, ANSWER_WAIT, |r| request.answer(r).is_some())?;
        Ok(report.and_then(|report| request.answer(&report).map(<[u8]>::to_vec)))
    }
}
