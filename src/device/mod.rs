//! The ATK Phantom Ultimate: finding it, reading what it has and switching profiles. All it is
//! asked is a read, but `Phantom::activate`.

mod hid;
mod protocol;

use std::time::Duration;

use protocol::{FEATURE_FPS_13K, REPORT_ID, Request};
pub use protocol::{Sensor, Slot};

/// Vendors of ATK's interfaces.
const VENDORS: [u16; 2] = [0x373B, 0x3554];
/// The command interface of BITMOUSE mice: usage page and usage.
const INTERFACE: (u16, u16) = (0xFF05, 1);
/// The wired Phantom; any other product with that interface is a receiver.
const WIRED: [u16; 2] = [0x12CE, 0x1312];
/// How long an answer is waited for (the receiver takes longer than the cable).
const ANSWER_WAIT: Duration = Duration::from_millis(1500);

/// The mouse's connection stopped answering: it was unplugged or its receiver went away.
#[derive(Debug)]
pub struct Disconnected;

pub type Result<T> = std::result::Result<T, Disconnected>;

/// How the mouse is reached: by its cable, or through its receiver.
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

/// What a profile has.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Profile {
    pub polling_hz: Option<u32>,
    /// Horizontal and vertical DPI of the active level.
    pub dpi: Option<(u16, u16)>,
    pub sensor: Option<Sensor>,
}

/// What is read from the mouse at one moment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snapshot {
    pub slot: Slot,
    pub profile: Profile,
    pub battery: Option<Battery>,
}

pub struct Phantom {
    link: hid::Link,
    connection: Connection,
    /// Whether the sensor mode is in the profile's memory (from the mouse's features, asked once
    /// per connection).
    sensor_in_memory: Option<bool>,
}

impl Phantom {
    /// The first ATK interface that answers which profile is active.
    pub fn find() -> Option<Phantom> {
        hid::interfaces()
            .into_iter()
            .filter(|i| VENDORS.contains(&i.vendor) && (i.usage_page, i.usage) == INTERFACE)
            .filter(|i| i.output_len > protocol::PACKET_LEN)
            .find_map(|interface| {
                let connection =
                    if WIRED.contains(&interface.product) { Connection::Wired } else { Connection::Wireless };
                let mut phantom = Phantom { link: hid::Link::open(&interface)?, connection, sensor_in_memory: None };
                matches!(phantom.active_slot(), Ok(Some(_))).then_some(phantom)
            })
    }

    pub fn connection(&self) -> Connection {
        self.connection
    }

    /// The active profile, or `None` if the mouse did not answer.
    pub fn active_slot(&mut self) -> Result<Option<Slot>> {
        Ok(self.ask(Request::Active)?.and_then(|data| protocol::active(&data)).map(|(slot, _)| slot))
    }

    /// Activates `slot`; `false` if the mouse did not confirm it.
    pub fn activate(&mut self, slot: Slot) -> Result<bool> {
        Ok(self.ask(Request::Activate(slot))?.is_some())
    }

    /// The active profile with what it has, and the battery; `None` if the mouse did not answer.
    pub fn snapshot(&mut self) -> Result<Option<Snapshot>> {
        let Some((slot, polling_hz)) = self.ask(Request::Active)?.and_then(|data| protocol::active(&data)) else {
            return Ok(None);
        };
        let profile = Profile { polling_hz, dpi: self.dpi()?, sensor: self.sensor()? };
        Ok(Some(Snapshot { slot, profile, battery: self.battery()? }))
    }

    /// The battery, or `None` if the mouse did not answer.
    pub fn battery(&mut self) -> Result<Option<Battery>> {
        let Some(percent) = self.ask(Request::Battery)?.and_then(|data| data.first().copied()) else {
            return Ok(None);
        };
        let charging = self.ask(Request::Charging)?.is_some_and(|data| data.first() == Some(&1));
        Ok(Some(Battery { percent, charging }))
    }

    /// The DPI of the active level: the level in the table, then only its entry.
    fn dpi(&mut self) -> Result<Option<(u16, u16)>> {
        let Some(level) = self.read_memory(protocol::DPI_TABLE, 1)? else { return Ok(None) };
        Ok(self.read_memory(protocol::dpi_entry(level[0]), 4)?.and_then(|entry| protocol::dpi(&entry)))
    }

    /// The sensor mode; `None` when this mouse keeps it somewhere this app does not know.
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

    /// Makes a request and returns its answer's data.
    fn ask(&mut self, request: Request) -> Result<Option<Vec<u8>>> {
        let packet = request.packet(self.connection);
        self.link.exchange(REPORT_ID, &packet, ANSWER_WAIT, |report| request.answer(report).map(<[u8]>::to_vec))
    }
}
