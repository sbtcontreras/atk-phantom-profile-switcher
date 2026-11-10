//! El ATK Phantom Ultimate (firmware BITMOUSE) por HID, con los mismos comandos que ATK HUB
//! (hub.atk.pro 3.2.32).
//!
//! Interfaz 0xFF05 / uso 1, reporte 8, paquetes de 63 bytes:
//! `[checksum] [0x72] [largo de parámetros] [secuencia] [destino] [comando] [largo] [datos…]`.
//! La respuesta, sin el número de reporte, repite el comando en el byte 4 y trae sus datos desde
//! el 6. El destino es 0 para el mouse por cable (PID 4814 o 4882) y 1 para el mouse a través de
//! su receptor; con 0, el receptor responde por sí mismo, todo en cero.
//!
//! Solo usa lecturas y el cambio de configuración (comando 10). La configuración va de 0 a 3 en el
//! mouse y de 1 a 4 en ATK HUB.

use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FlushQueue, HidD_FreePreparsedData,
    HidD_GetAttributes, HidD_GetHidGuid, HidD_GetPreparsedData, HidP_GetCaps,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};

const REPORT_ID: u8 = 8;
const VENDORS: [u16; 2] = [0x373B, 0x3554];
const WIRED: [u16; 2] = [4814, 4882];
const ANSWER_WAIT: Duration = Duration::from_millis(1500);
/// Donde empiezan los datos de una respuesta.
const DATA: usize = 6;

const GET_BATTERY_LEVEL: u8 = 7;
const GET_CONFIG: u8 = 9;
const SET_CONFIG: u8 = 10;
const GET_CHARGING: u8 = 15;
const GET_ADDRESS_DATA: u8 = 23;
const GET_FEATURES: u8 = 177;
/// Bit de las funciones: el mouse tiene el modo de 13K, cuyo modo de sensor se lee de otra forma.
const FEATURE_FPS_13K: u8 = 8;
/// La tabla de DPI de la configuración activa: nivel activo, cantidad de niveles y, por nivel,
/// 8 bytes (DPI horizontal y vertical, 16 bits cada uno, y su color).
const DPI_TABLE: u16 = 1;
/// El modo del sensor de la configuración activa («Sensor Sampling Rate» en ATK HUB).
const SENSOR_MODE: u16 = 74;

/// El mouse dejó de responder a la conexión (desconectado o apagado).
#[derive(Debug)]
pub struct Gone;

/// El modo del sensor, como lo llama ATK HUB.
#[derive(Clone, Copy)]
pub enum Sensor {
    /// «Basic Mode»: bajo consumo.
    Basic,
    /// «ATK Shard Competitive Firmware».
    Competitive,
    /// «ATK Shard Competitive Firmware MAX».
    Max,
}

/// Lo que se lee del mouse en un momento.
pub struct Snapshot {
    /// Configuración activa, de 1 a 4.
    pub config: u8,
    pub rate_hz: Option<u32>,
    /// DPI del nivel activo: horizontal y vertical.
    pub dpi: Option<(u16, u16)>,
    pub sensor: Option<Sensor>,
    /// Batería en porcentaje y si está cargando.
    pub battery: Option<(u8, bool)>,
}

struct Info {
    vendor: u16,
    product: u16,
    page: u16,
    usage: u16,
    input_len: usize,
    output_len: usize,
}

pub struct Phantom {
    handle: HANDLE,
    event: HANDLE,
    target: u8,
    input_len: usize,
    output_len: usize,
    /// Si el modo del sensor está en la dirección 74: según las funciones del mouse, preguntadas
    /// una vez por conexión.
    sensor_at_74: Option<bool>,
}

// SAFETY: los handles son de este objeto y solo los usa el hilo que lo tiene.
unsafe impl Send for Phantom {}

impl Phantom {
    /// La primera interfaz ATK que responde a la pregunta de su configuración.
    pub fn find() -> Option<Phantom> {
        for path in hid_paths() {
            let Some(info) = describe(&path) else { continue };
            if !VENDORS.contains(&info.vendor) || (info.page, info.usage) != (0xFF05, 1) || info.output_len < 64 {
                continue;
            }
            let Some(mut mouse) = Phantom::open(&path, &info) else { continue };
            if let Ok(Some(_)) = mouse.config() {
                return Some(mouse);
            }
        }
        None
    }

    fn open(path: &[u16], info: &Info) -> Option<Phantom> {
        // SAFETY: `path` termina en 0; los handles se cierran en `drop`.
        unsafe {
            let handle = CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            );
            if handle == INVALID_HANDLE_VALUE {
                return None;
            }
            let event = CreateEventW(null(), 1, 0, null());
            if event.is_null() {
                CloseHandle(handle);
                return None;
            }
            Some(Phantom {
                handle,
                event,
                target: if WIRED.contains(&info.product) { 0 } else { 1 },
                input_len: info.input_len.max(64),
                output_len: info.output_len,
                sensor_at_74: None,
            })
        }
    }

    /// La configuración activa, de 1 a 4, o `None` si el mouse no contestó.
    pub fn config(&mut self) -> Result<Option<u8>, Gone> {
        Ok(self.ask(GET_CONFIG, 20, 18, &[])?.map(|r| r[DATA].saturating_add(1)))
    }

    /// Activa la configuración `number` (1 a 4); `false` si el mouse no lo confirmó.
    pub fn set_config(&mut self, number: u8) -> Result<bool, Gone> {
        Ok(self.ask(SET_CONFIG, 3, 1, &[number - 1])?.is_some())
    }

    /// Configuración, polling rate, DPI, modo del sensor y batería; `None` si el mouse no contestó.
    pub fn snapshot(&mut self) -> Result<Option<Snapshot>, Gone> {
        let Some(config) = self.ask(GET_CONFIG, 20, 18, &[])? else { return Ok(None) };
        let rate_hz = match config[DATA + 2] {
            0 => Some(1000),
            1 => Some(500),
            2 => Some(250),
            3 => Some(125),
            4 => Some(8000),
            5 => Some(4000),
            6 => Some(2000),
            _ => None,
        };
        let dpi = match self.read_address(DPI_TABLE, 1)? {
            Some(level) => self
                .read_address(DPI_TABLE + 2 + 8 * level[0] as u16, 4)?
                .map(|d| (u16::from_le_bytes([d[0], d[1]]), u16::from_le_bytes([d[2], d[3]]))),
            None => None,
        };
        if self.sensor_at_74.is_none() {
            self.sensor_at_74 = self.ask(GET_FEATURES, 3, 1, &[])?.map(|r| r[DATA] & FEATURE_FPS_13K == 0);
        }
        let sensor = match self.sensor_at_74 {
            Some(true) => self.read_address(SENSOR_MODE, 1)?.and_then(|mode| match mode[0] {
                0 => Some(Sensor::Basic),
                4 => Some(Sensor::Competitive),
                5 => Some(Sensor::Max),
                _ => None,
            }),
            _ => None,
        };
        let battery = self.battery()?;
        Ok(Some(Snapshot {
            config: config[DATA].saturating_add(1),
            rate_hz,
            dpi,
            sensor,
            battery,
        }))
    }

    /// Batería en porcentaje y si está cargando; `None` si el mouse no contestó.
    pub fn battery(&mut self) -> Result<Option<(u8, bool)>, Gone> {
        let Some(level) = self.ask(GET_BATTERY_LEVEL, 2, 1, &[])?.map(|r| r[DATA]) else { return Ok(None) };
        let charging = self.ask(GET_CHARGING, 2, 1, &[])?.is_some_and(|r| r[DATA] == 1);
        Ok(Some((level, charging)))
    }

    /// Si se le habla a través del receptor (si no, por cable).
    pub fn wireless(&self) -> bool {
        self.target == 1
    }

    /// `len` bytes (hasta 10) de la memoria de la configuración activa, desde `address`.
    fn read_address(&mut self, address: u16, len: u8) -> Result<Option<Vec<u8>>, Gone> {
        let [low, high] = address.to_le_bytes();
        let answer = self.ask(GET_ADDRESS_DATA, 10, 13, &[low, high, len])?;
        let start = DATA + 3;
        Ok(answer.filter(|r| r.len() >= start + len as usize).map(|r| r[start..start + len as usize].to_vec()))
    }

    /// Envía un comando y espera su respuesta (sin el número de reporte).
    fn ask(&mut self, command: u8, param_len: u8, cmd_len: u8, data: &[u8]) -> Result<Option<Vec<u8>>, Gone> {
        // SAFETY: handle abierto por `open`.
        unsafe { HidD_FlushQueue(self.handle) };
        let mut report = vec![0u8; self.output_len];
        report[0] = REPORT_ID;
        report[1..64].copy_from_slice(&packet(self.target, command, param_len, cmd_len, data));
        self.write(&report)?;

        let deadline = Instant::now() + ANSWER_WAIT;
        let mut buffer = vec![0u8; self.input_len];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            if let Some(read) = self.read(&mut buffer, left)? {
                // Windows antepone el número de reporte; el aviso de espera del receptor no
                // repite el comando.
                let answer = &buffer[1..read.max(1)];
                if answer.len() > DATA && answer[4] == command {
                    return Ok(Some(answer.to_vec()));
                }
            }
        }
    }

    fn write(&mut self, data: &[u8]) -> Result<(), Gone> {
        // SAFETY: `overlapped` y `data` viven hasta que la operación termina o se cancela.
        unsafe {
            let mut overlapped: OVERLAPPED = zeroed();
            overlapped.hEvent = self.event;
            ResetEvent(self.event);
            if WriteFile(self.handle, data.as_ptr(), data.len() as u32, null_mut(), &mut overlapped) == 0
                && GetLastError() != ERROR_IO_PENDING
            {
                return Err(Gone);
            }
            let mut done = 0u32;
            if WaitForSingleObject(self.event, 1000) != WAIT_OBJECT_0 {
                CancelIoEx(self.handle, &overlapped);
                GetOverlappedResult(self.handle, &overlapped, &mut done, 1);
                return Err(Gone);
            }
            if GetOverlappedResult(self.handle, &overlapped, &mut done, 0) == 0 {
                return Err(Gone);
            }
        }
        Ok(())
    }

    /// Un reporte de entrada, o `None` si no llegó ninguno a tiempo.
    fn read(&mut self, buffer: &mut [u8], wait: Duration) -> Result<Option<usize>, Gone> {
        // SAFETY: `overlapped` y `buffer` viven hasta que la operación termina o se cancela.
        unsafe {
            let mut overlapped: OVERLAPPED = zeroed();
            overlapped.hEvent = self.event;
            ResetEvent(self.event);
            if ReadFile(self.handle, buffer.as_mut_ptr(), buffer.len() as u32, null_mut(), &mut overlapped) == 0
                && GetLastError() != ERROR_IO_PENDING
            {
                return Err(Gone);
            }
            let mut done = 0u32;
            let millis = wait.as_millis().clamp(1, u32::MAX as u128) as u32;
            if WaitForSingleObject(self.event, millis) != WAIT_OBJECT_0 {
                CancelIoEx(self.handle, &overlapped);
                // Si el reporte llegó justo antes de cancelar, se aprovecha.
                return Ok((GetOverlappedResult(self.handle, &overlapped, &mut done, 1) != 0).then_some(done as usize));
            }
            if GetOverlappedResult(self.handle, &overlapped, &mut done, 0) == 0 {
                return Err(Gone);
            }
            Ok(Some(done as usize))
        }
    }
}

impl Drop for Phantom {
    fn drop(&mut self) {
        // SAFETY: handles abiertos por `open`, cerrados solo aquí.
        unsafe {
            CancelIoEx(self.handle, null());
            CloseHandle(self.handle);
            CloseHandle(self.event);
        }
    }
}

fn packet(target: u8, command: u8, param_len: u8, cmd_len: u8, data: &[u8]) -> [u8; 63] {
    let mut p = [0u8; 63];
    p[1] = 114;
    p[2] = param_len;
    p[4] = target;
    p[5] = command;
    p[6] = cmd_len;
    p[7..7 + data.len()].copy_from_slice(data);
    p[0] = p[1..].iter().fold(0u8, |sum, b| sum.wrapping_add(*b));
    p
}

/// Las rutas (terminadas en 0) de las interfaces HID presentes.
fn hid_paths() -> Vec<Vec<u16>> {
    let mut paths = Vec::new();
    // SAFETY: la lista se destruye al final; el detalle se lee dentro de su búfer alineado.
    unsafe {
        let mut guid = zeroed();
        HidD_GetHidGuid(&mut guid);
        let set = SetupDiGetClassDevsW(&guid, null(), null_mut(), DIGCF_PRESENT | DIGCF_DEVICEINTERFACE);
        if set == INVALID_HANDLE_VALUE as isize {
            return paths;
        }
        let mut data: SP_DEVICE_INTERFACE_DATA = zeroed();
        data.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
        let mut index = 0;
        while SetupDiEnumDeviceInterfaces(set, null(), &guid, index, &mut data) != 0 {
            index += 1;
            let mut required = 0u32;
            SetupDiGetDeviceInterfaceDetailW(set, &data, null_mut(), 0, &mut required, null_mut());
            if required < 8 {
                continue;
            }
            let mut buffer = vec![0u32; (required as usize).div_ceil(4)];
            let detail = buffer.as_mut_ptr().cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &data, detail, required, null_mut(), null_mut()) == 0 {
                continue;
            }
            let start = (&raw const (*detail).DevicePath).cast::<u16>();
            let max = (required as usize - 4) / 2;
            let text = std::slice::from_raw_parts(start, max);
            let len = text.iter().position(|c| *c == 0).unwrap_or(max);
            let mut path = text[..len].to_vec();
            path.push(0);
            paths.push(path);
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    paths
}

/// Fabricante, producto, interfaz y largo de reportes de una interfaz HID, sin abrirla para leer.
fn describe(path: &[u16]) -> Option<Info> {
    // SAFETY: `path` termina en 0; el handle y los datos preparados se liberan antes de salir.
    unsafe {
        let handle = CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut info = None;
        let mut attributes: HIDD_ATTRIBUTES = zeroed();
        attributes.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
        let mut preparsed = 0;
        if HidD_GetAttributes(handle, &mut attributes) && HidD_GetPreparsedData(handle, &mut preparsed) {
            let mut caps: HIDP_CAPS = zeroed();
            if HidP_GetCaps(preparsed, &mut caps) == HIDP_STATUS_SUCCESS {
                info = Some(Info {
                    vendor: attributes.VendorID,
                    product: attributes.ProductID,
                    page: caps.UsagePage,
                    usage: caps.Usage,
                    input_len: caps.InputReportByteLength as usize,
                    output_len: caps.OutputReportByteLength as usize,
                });
            }
            HidD_FreePreparsedData(preparsed);
        }
        CloseHandle(handle);
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets_match_what_the_mouse_answered() {
        // Los dos paquetes de tus pruebas, a los que el mouse respondió.
        let wired = packet(0, GET_CONFIG, 20, 18, &[]);
        assert_eq!(&wired[..8], &[0xA1, 114, 20, 0, 0, 9, 18, 0]);
        let receiver = packet(1, GET_CONFIG, 20, 18, &[]);
        assert_eq!(&receiver[..8], &[0xA2, 114, 20, 0, 1, 9, 18, 0]);
        assert!(wired[8..].iter().all(|b| *b == 0));
    }

    #[test]
    fn set_config_carries_the_index() {
        let p = packet(1, SET_CONFIG, 3, 1, &[1]);
        assert_eq!(&p[..8], &[0x82, 114, 3, 0, 1, 10, 1, 1]);
    }
}
