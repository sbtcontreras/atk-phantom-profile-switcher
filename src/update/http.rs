//! Asking GitHub, through WinHTTP (part of Windows): where a page leads, read without following
//! it, and what a file holds, following GitHub to where it keeps it.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_COOKIES,
    WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
    WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
};

use super::{Unreachable, Version};
use crate::APP_ID;
use crate::win::wide;

pub const SITE: &str = "https://github.com";
/// How long each step waits (finding GitHub, connecting, each part of its answer).
const TIMEOUT_MS: i32 = 15_000;

/// Where `url` leads, read without following it; `None` when GitHub answers without a redirect.
pub fn redirect(url: &str) -> Result<Option<String>, Unreachable> {
    let exchange = Exchange::send("HEAD", url, false)?;
    if !(300..400).contains(&exchange.status()?) {
        return Ok(None);
    }
    Ok(exchange.location())
}

/// What the file at `url` holds; `None` when it is not there, or is larger than `limit`.
pub fn download(url: &str, limit: usize) -> Result<Option<Vec<u8>>, Unreachable> {
    let exchange = Exchange::send("GET", url, true)?;
    if exchange.status()? != 200 {
        return Ok(None);
    }
    exchange.body(limit)
}

/// A request and its answer. Its handles close in order: the request before the connection
/// before the session.
struct Exchange {
    request: Handle,
    _connection: Handle,
    _session: Handle,
}

impl Exchange {
    fn send(method: &str, url: &str, follow: bool) -> Result<Exchange, Unreachable> {
        let path = url.strip_prefix(SITE).ok_or(Unreachable)?;
        let host = SITE.trim_start_matches("https://");
        let agent = wide(&format!("{APP_ID}/{}", Version::current()));
        // SAFETY: NUL-terminated strings that live through each call; each handle goes into a
        // `Handle`, closed when dropped.
        unsafe {
            let session =
                Handle::new(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0))?;
            WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS);
            let connection =
                Handle::new(WinHttpConnect(session.0, wide(host).as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0))?;
            let request = Handle::new(WinHttpOpenRequest(
                connection.0,
                wide(method).as_ptr(),
                wide(path).as_ptr(),
                null(),
                null(),
                null(),
                WINHTTP_FLAG_SECURE,
            ))?;
            let features: u32 = if follow { 0 } else { WINHTTP_DISABLE_REDIRECTS } | WINHTTP_DISABLE_COOKIES;
            WinHttpSetOption(request.0, WINHTTP_OPTION_DISABLE_FEATURE, (&raw const features).cast(), 4);
            if WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) == 0
                || WinHttpReceiveResponse(request.0, null_mut()) == 0
            {
                return Err(Unreachable);
            }
            Ok(Exchange { request, _connection: connection, _session: session })
        }
    }

    fn status(&self) -> Result<u32, Unreachable> {
        let mut status = 0u32;
        let mut size = size_of::<u32>() as u32;
        let flags = WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER;
        // SAFETY: a request with its answer, and room for the number.
        let read = unsafe {
            WinHttpQueryHeaders(self.request.0, flags, null(), (&raw mut status).cast(), &mut size, null_mut())
        };
        if read == 0 { Err(Unreachable) } else { Ok(status) }
    }

    fn location(&self) -> Option<String> {
        let mut location = [0u16; 512];
        let mut size = size_of_val(&location) as u32;
        // SAFETY: a request with its answer, and `size` bytes of room.
        let read = unsafe {
            WinHttpQueryHeaders(
                self.request.0,
                WINHTTP_QUERY_LOCATION,
                null(),
                location.as_mut_ptr().cast(),
                &mut size,
                null_mut(),
            )
        };
        let units = (size as usize / 2).min(location.len());
        (read != 0).then(|| String::from_utf16_lossy(&location[..units]))
    }

    fn body(&self, limit: usize) -> Result<Option<Vec<u8>>, Unreachable> {
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            // SAFETY: a request with its answer.
            if unsafe { WinHttpQueryDataAvailable(self.request.0, &mut available) } == 0 {
                return Err(Unreachable);
            }
            if available == 0 {
                return Ok(Some(body));
            }
            let start = body.len();
            if start + available as usize > limit {
                return Ok(None);
            }
            body.resize(start + available as usize, 0);
            let mut read = 0u32;
            // SAFETY: `available` bytes of room from `start`.
            let ok =
                unsafe { WinHttpReadData(self.request.0, body[start..].as_mut_ptr().cast(), available, &mut read) };
            if ok == 0 {
                return Err(Unreachable);
            }
            body.truncate(start + read as usize);
        }
    }
}

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut c_void);

impl Handle {
    fn new(handle: *mut c_void) -> Result<Handle, Unreachable> {
        if handle.is_null() { Err(Unreachable) } else { Ok(Handle(handle)) }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: a handle WinHTTP opened, closed once.
        unsafe { WinHttpCloseHandle(self.0) };
    }
}
