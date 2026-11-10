//! The protocol of the ATK Phantom Ultimate (BITMOUSE firmware), as ATK HUB uses it: how each
//! request is built and how its answer is read. No I/O, so it is tested with real answers.
//!
//! Each request is report 8, 63 bytes long:
//!
//! ```text
//! [checksum] [0x72] [parameter length] [sequence] [route] [command] [length] [data…]
//! ```
//!
//! The checksum is the sum of bytes 1 to 62. The route is 0 for the wired mouse and 1 for the
//! mouse through its receiver: with 0, the receiver answers for itself, all zeros. The answer
//! (without the report number) repeats the command in byte 4 and carries its data from byte 6;
//! the receiver first sends a wait notice (`72 02 …`) that does not repeat it.

pub const REPORT_ID: u8 = 8;
pub const PACKET_LEN: usize = 63;

const HEADER: u8 = 0x72;
/// Where an answer's data starts.
const DATA: usize = 6;

/// Where the DPI table is in the active profile's memory: the active level, the number of
/// levels and, per level, 8 bytes (16-bit horizontal and vertical DPI, and its color).
pub const DPI_TABLE: u16 = 1;
/// Where the active profile's sensor mode ("Sensor Sampling Rate" in ATK HUB) is.
pub const SENSOR_MODE: u16 = 74;
/// Bit of the mouse's features: it has the 13K mode, and then the sensor mode is not at
/// `SENSOR_MODE`.
pub const FEATURE_FPS_13K: u8 = 8;

/// One of the mouse's 4 profiles: 1 to 4, as in ATK HUB (0 to 3 in the mouse).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot(u8);

impl Slot {
    pub const ALL: [Slot; 4] = [Slot(1), Slot(2), Slot(3), Slot(4)];

    pub fn new(number: u8) -> Option<Slot> {
        (1..=4).contains(&number).then_some(Slot(number))
    }

    fn from_index(index: u8) -> Option<Slot> {
        Slot::new(index.checked_add(1)?)
    }

    pub fn number(self) -> u8 {
        self.0
    }

    pub fn index(self) -> usize {
        usize::from(self.0 - 1)
    }
}

/// How the mouse is reached.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    Wired,
    Receiver,
}

/// What the mouse can be asked. Everything is a read, but `Activate`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Request {
    /// The active profile and its polling rate.
    Active,
    Activate(Slot),
    Battery,
    Charging,
    Features,
    /// Up to 10 bytes of the active profile's memory.
    Memory {
        address: u16,
        len: u8,
    },
}

impl Request {
    fn command(self) -> u8 {
        match self {
            Request::Battery => 7,
            Request::Active => 9,
            Request::Activate(_) => 10,
            Request::Charging => 15,
            Request::Memory { .. } => 23,
            Request::Features => 177,
        }
    }

    /// Parameter length, command length and data, as ATK HUB builds them.
    fn parameters(self) -> (u8, u8, Vec<u8>) {
        match self {
            Request::Active => (20, 18, vec![]),
            Request::Activate(slot) => (3, 1, vec![slot.0 - 1]),
            Request::Battery | Request::Charging => (2, 1, vec![]),
            Request::Features => (3, 1, vec![]),
            Request::Memory { address, len } => {
                let [low, high] = address.to_le_bytes();
                (10, 13, vec![low, high, len])
            }
        }
    }

    /// The request, without the report number.
    pub fn packet(self, route: Route) -> [u8; PACKET_LEN] {
        let (param_len, cmd_len, data) = self.parameters();
        let mut p = [0u8; PACKET_LEN];
        p[1] = HEADER;
        p[2] = param_len;
        p[4] = match route {
            Route::Wired => 0,
            Route::Receiver => 1,
        };
        p[5] = self.command();
        p[6] = cmd_len;
        p[7..7 + data.len()].copy_from_slice(&data);
        p[0] = p[1..].iter().fold(0u8, |sum, b| sum.wrapping_add(*b));
        p
    }

    /// The data of `report` (without the report number) if it answers this request.
    pub fn answer(self, report: &[u8]) -> Option<&[u8]> {
        let command = self.command();
        (report.len() > DATA && report[4] == command).then(|| &report[DATA..])
    }
}

/// The active profile and its polling rate, from the answer to `Request::Active`.
pub fn active(data: &[u8]) -> Option<(Slot, Option<u32>)> {
    let slot = Slot::from_index(*data.first()?)?;
    let polling_hz = data.get(2).and_then(|code| match code {
        0 => Some(1000),
        1 => Some(500),
        2 => Some(250),
        3 => Some(125),
        4 => Some(8000),
        5 => Some(4000),
        6 => Some(2000),
        _ => None,
    });
    Some((slot, polling_hz))
}

/// The bytes read from memory, from the answer to `Request::Memory`.
pub fn memory(data: &[u8], len: u8) -> Option<&[u8]> {
    data.get(3..3 + usize::from(len))
}

/// Where, in the DPI table, the DPI of level `level` is.
pub fn dpi_entry(level: u8) -> u16 {
    DPI_TABLE + 2 + 8 * u16::from(level)
}

/// Horizontal and vertical DPI from the first 4 bytes of a level of the table.
pub fn dpi(entry: &[u8]) -> Option<(u16, u16)> {
    match entry {
        [x0, x1, y0, y1, ..] => Some((u16::from_le_bytes([*x0, *x1]), u16::from_le_bytes([*y0, *y1]))),
        _ => None,
    }
}

/// The sensor mode, as ATK HUB names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sensor {
    /// "Basic Mode": low power.
    Basic,
    /// "ATK Shard Competitive Firmware".
    Competitive,
    /// "ATK Shard Competitive Firmware MAX".
    Max,
}

/// The sensor mode stored at `SENSOR_MODE`.
pub fn sensor(mode: u8) -> Option<Sensor> {
    match mode {
        0 => Some(Sensor::Basic),
        4 => Some(Sensor::Competitive),
        5 => Some(Sensor::Max),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer captured with hub.atk.pro, as hexadecimal text.
    fn report(hex: &str) -> Vec<u8> {
        let mut bytes: Vec<u8> = hex.split_whitespace().filter_map(|b| u8::from_str_radix(b, 16).ok()).collect();
        bytes.resize(PACKET_LEN, 0);
        bytes
    }

    #[test]
    fn packets_are_the_ones_the_mouse_answered() {
        let wired = Request::Active.packet(Route::Wired);
        assert_eq!(&wired[..8], &[0xA1, 0x72, 20, 0, 0, 9, 18, 0]);
        let receiver = Request::Active.packet(Route::Receiver);
        assert_eq!(&receiver[..8], &[0xA2, 0x72, 20, 0, 1, 9, 18, 0]);
        assert!(receiver[8..].iter().all(|b| *b == 0));
    }

    #[test]
    fn activating_carries_the_slot_index() {
        let p = Request::Activate(Slot(2)).packet(Route::Receiver);
        assert_eq!(&p[..8], &[0x82, 0x72, 3, 0, 1, 10, 1, 1]);
    }

    #[test]
    fn reads_the_active_profile_and_its_polling_rate() {
        let first = report("72 00 14 00 09 12 00 00 00 01 00 05 00 00 00 3c 00 01");
        let second = report("72 00 14 00 09 12 01 00 05 01 00 05 00 00 00 3c 00 01");
        let data = |r| Request::Active.answer(r).and_then(active);
        assert_eq!(data(&first), Some((Slot(1), Some(1000))));
        assert_eq!(data(&second), Some((Slot(2), Some(4000))));
    }

    #[test]
    fn skips_the_receivers_wait_notice() {
        assert_eq!(Request::Active.answer(&report("72 02 00 00 00 00")), None);
    }

    #[test]
    fn reads_the_battery() {
        let level = report("72 00 03 00 07 01 5a");
        let charging = report("72 00 03 00 0f 01 00");
        assert_eq!(Request::Battery.answer(&level).map(|d| d[0]), Some(90));
        assert_eq!(Request::Charging.answer(&charging).map(|d| d[0]), Some(0));
        assert_eq!(Request::Battery.answer(&charging), None);
    }

    #[test]
    fn reads_the_dpi_of_the_active_level() {
        // The DPI table read from the user's mouse: a single level, 1600 × 1600.
        let table = report("00 08 40 06 40 06 ff 00 00 00 00 00 00 00 ff 00");
        assert_eq!(table[0], 0);
        let entry = usize::from(dpi_entry(table[0]) - DPI_TABLE);
        assert_eq!(dpi(&table[entry..]), Some((1600, 1600)));
    }

    #[test]
    fn reads_memory_after_its_address_and_length() {
        let answer = report("72 00 0a 00 17 0d 4a 00 01 04");
        let data = Request::Memory { address: SENSOR_MODE, len: 1 }.answer(&answer);
        assert_eq!(data.and_then(|d| memory(d, 1)), Some(&[4u8][..]));
        assert_eq!(sensor(4), Some(Sensor::Competitive));
        assert_eq!(sensor(0), Some(Sensor::Basic));
    }

    #[test]
    fn slots_are_one_to_four() {
        assert_eq!(Slot::new(0), None);
        assert_eq!(Slot::new(5), None);
        assert_eq!(Slot::from_index(3), Some(Slot(4)));
        assert_eq!(Slot::from_index(255), None);
    }
}
