//! Windows' HID interfaces: which there are, what they are, and how to send one an output report
//! and wait, with a deadline, for the input report that answers it.

use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FlushQueue, HidD_FreePreparsedData, HidD_GetAttributes,
    HidD_GetHidGuid, HidD_GetPreparsedData, HidP_GetCaps,
};
use windows_sys::Win32::Foundation::{
    ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};

use super::Disconnected;

/// The longest a report may take to be sent.
const WRITE_WAIT_MS: u32 = 1000;

/// A present HID interface: its path and what it says about itself.
pub struct Interface {
    /// NUL-terminated, for `CreateFileW`.
    path: Vec<u16>,
    pub vendor: u16,
    pub product: u16,
    pub usage_page: u16,
    pub usage: u16,
    input_len: usize,
    pub output_len: usize,
}

/// Every present HID interface that can be described.
pub fn interfaces() -> Vec<Interface> {
    paths().into_iter().filter_map(describe).collect()
}

/// An interface opened to talk to it.
pub struct Link {
    file: OwnedHandle,
    /// The signal that each read or write is done (overlapped I/O).
    event: OwnedHandle,
    input_len: usize,
    output_len: usize,
}

impl Link {
    pub fn open(interface: &Interface) -> Option<Link> {
        let file = open_file(&interface.path, GENERIC_READ | GENERIC_WRITE, FILE_FLAG_OVERLAPPED)?;
        // SAFETY: creates an unnamed event; the handle goes into an `OwnedHandle`.
        let event = unsafe { CreateEventW(null(), 1, 0, null()) };
        if event.is_null() {
            return None;
        }
        Some(Link {
            file,
            // SAFETY: `event` is a valid handle just created, owned by nobody else.
            event: unsafe { OwnedHandle::from_raw_handle(event) },
            input_len: interface.input_len,
            output_len: interface.output_len,
        })
    }

    /// Sends `packet` as report `report_id` and returns the first input report that `is_answer`
    /// accepts (without the report number), or `None` if none arrives within `wait`.
    pub fn exchange(
        &mut self,
        report_id: u8,
        packet: &[u8],
        wait: Duration,
        is_answer: impl Fn(&[u8]) -> bool,
    ) -> Result<Option<Vec<u8>>, Disconnected> {
        // SAFETY: a handle opened by `open`. Drops old answers left in the queue.
        unsafe { HidD_FlushQueue(self.raw()) };
        let mut report = vec![0u8; self.output_len.max(packet.len() + 1)];
        report[0] = report_id;
        report[1..=packet.len()].copy_from_slice(packet);
        self.write(&report)?;

        let deadline = Instant::now() + wait;
        let mut buffer = vec![0u8; self.input_len.max(2)];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            if let Some(read) = self.read(&mut buffer, left)? {
                // Windows puts the report number first.
                let report = &buffer[1..read.max(1)];
                if is_answer(report) {
                    return Ok(Some(report.to_vec()));
                }
            }
        }
    }

    fn raw(&self) -> HANDLE {
        self.file.as_raw_handle()
    }

    fn write(&mut self, report: &[u8]) -> Result<(), Disconnected> {
        let mut overlapped = self.overlapped();
        // SAFETY: `overlapped` and `report` live until the write ends or is cancelled.
        let started =
            unsafe { WriteFile(self.raw(), report.as_ptr(), report.len() as u32, null_mut(), &mut overlapped) };
        match self.finish(started, &overlapped, WRITE_WAIT_MS)? {
            Some(_) => Ok(()),
            None => Err(Disconnected),
        }
    }

    /// An input report, or `None` if none arrived within `wait`.
    fn read(&mut self, buffer: &mut [u8], wait: Duration) -> Result<Option<usize>, Disconnected> {
        let mut overlapped = self.overlapped();
        // SAFETY: `overlapped` and `buffer` live until the read ends or is cancelled.
        let started =
            unsafe { ReadFile(self.raw(), buffer.as_mut_ptr(), buffer.len() as u32, null_mut(), &mut overlapped) };
        let millis = wait.as_millis().clamp(1, u128::from(u32::MAX)) as u32;
        self.finish(started, &overlapped, millis)
    }

    fn overlapped(&self) -> OVERLAPPED {
        // SAFETY: OVERLAPPED is plain data; all zeros is its initial state.
        let mut overlapped: OVERLAPPED = unsafe { zeroed() };
        overlapped.hEvent = self.event.as_raw_handle();
        // SAFETY: the event belongs to this `Link`.
        unsafe { ResetEvent(overlapped.hEvent) };
        overlapped
    }

    /// Waits for an overlapped operation: its bytes, `None` if it did not end in time (and is
    /// cancelled), or `Disconnected` if it failed.
    fn finish(&self, started: i32, overlapped: &OVERLAPPED, millis: u32) -> Result<Option<usize>, Disconnected> {
        // SAFETY: `overlapped` is that of the operation just started on this handle.
        unsafe {
            if started == 0 && GetLastError() != ERROR_IO_PENDING {
                return Err(Disconnected);
            }
            let mut done = 0u32;
            if WaitForSingleObject(overlapped.hEvent, millis) != WAIT_OBJECT_0 {
                CancelIoEx(self.raw(), overlapped);
                // If it ended just before being cancelled, its result is used.
                let finished = GetOverlappedResult(self.raw(), overlapped, &mut done, 1) != 0;
                return Ok(finished.then_some(done as usize));
            }
            if GetOverlappedResult(self.raw(), overlapped, &mut done, 0) == 0 {
                return Err(Disconnected);
            }
            Ok(Some(done as usize))
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        // SAFETY: cancels anything pending before the handle is closed.
        unsafe { CancelIoEx(self.raw(), null()) };
    }
}

fn open_file(path: &[u16], access: u32, flags: u32) -> Option<OwnedHandle> {
    // SAFETY: `path` is NUL-terminated; the handle goes into an `OwnedHandle`.
    let handle = unsafe {
        CreateFileW(path.as_ptr(), access, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, flags, null_mut())
    };
    // SAFETY: a valid handle just opened, owned by nobody else.
    (handle != INVALID_HANDLE_VALUE).then(|| unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// Vendor, product, interface and report lengths, without opening it to read or write.
fn describe(path: Vec<u16>) -> Option<Interface> {
    let file = open_file(&path, 0, 0)?;
    let handle = file.as_raw_handle();
    // SAFETY: initialized plain structures; the preparsed data is freed before returning.
    unsafe {
        let mut attributes: HIDD_ATTRIBUTES = zeroed();
        attributes.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
        let mut preparsed = 0;
        if !HidD_GetAttributes(handle, &mut attributes) || !HidD_GetPreparsedData(handle, &mut preparsed) {
            return None;
        }
        let mut caps: HIDP_CAPS = zeroed();
        let status = HidP_GetCaps(preparsed, &mut caps);
        HidD_FreePreparsedData(preparsed);
        (status == HIDP_STATUS_SUCCESS).then(|| Interface {
            path,
            vendor: attributes.VendorID,
            product: attributes.ProductID,
            usage_page: caps.UsagePage,
            usage: caps.Usage,
            input_len: usize::from(caps.InputReportByteLength),
            output_len: usize::from(caps.OutputReportByteLength),
        })
    }
}

/// The paths (NUL-terminated) of the present HID interfaces.
fn paths() -> Vec<Vec<u16>> {
    let mut paths = Vec::new();
    // SAFETY: the list is destroyed at the end; each detail is read within its aligned buffer.
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
