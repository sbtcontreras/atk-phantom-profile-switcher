//! Whether a newer Phantom Switch is out. It asks GitHub where the latest release's page leads:
//! one `HEAD` request, whose answer is a redirect to `…/releases/tag/vX.Y.Z`, read without
//! following it. Nothing is downloaded or installed, and nothing is sent but the request itself;
//! the menu offers the release's page to download it by hand.
//!
//! `releases/latest` leads only to stable releases: pre-releases are never offered.

use std::ffi::c_void;
use std::fmt;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_COOKIES,
    WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
    WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
    WinHttpSetTimeouts,
};
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::APP_ID;
use crate::win::wide;

/// The latest release's page, where the menu leads.
pub const RELEASE_PAGE: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases/latest");
const SITE: &str = "https://github.com";
/// How long a check waits for each step (finding GitHub, connecting, its answer).
const TIMEOUT_MS: i32 = 15_000;
/// At most one check a day.
const CHECK_EVERY_SECS: u64 = 24 * 60 * 60;
/// After a check that could not ask (no connection yet, say, right after signing in), the next
/// one waits an hour.
const RETRY_AFTER_SECS: u64 = 60 * 60;

/// A release's number: major, minor and patch.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version([u16; 3]);

impl Version {
    /// This very build's.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version([0; 3]))
    }

    /// `1.2.3` or `v1.2.3`; anything else (a pre-release's `-beta`, say) is not one.
    pub fn parse(text: &str) -> Option<Version> {
        let mut parts = text.strip_prefix('v').unwrap_or(text).split('.').map(|part| {
            let digits = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
            digits.then(|| part.parse().ok()).flatten()
        });
        let version = Version([parts.next()??, parts.next()??, parts.next()??]);
        parts.next().is_none().then_some(version)
    }

    pub fn is_newer(self) -> bool {
        self > Version::current()
    }

    /// Packed into a message's `lParam` (64 bits: the app is built only for x86-64).
    fn to_bits(self) -> isize {
        let [major, minor, patch] = self.0.map(u64::from);
        (major << 32 | minor << 16 | patch) as isize
    }

    fn from_bits(bits: isize) -> Version {
        Version([(bits >> 32) as u16, (bits >> 16) as u16, bits as u16])
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [major, minor, patch] = self.0;
        write!(f, "{major}.{minor}.{patch}")
    }
}

/// GitHub could not be asked: no connection, or it did not answer in time.
#[derive(Debug, PartialEq, Eq)]
pub struct Unreachable;

/// What a check found: the latest stable release, or `None` when there is none.
pub type Answer = Result<Option<Version>, Unreachable>;

/// Whether a check is due, the last one that answered having been at `answered`, and the last
/// one that could not ask at `failed` (seconds since 1970).
pub fn due(answered: Option<u64>, failed: Option<u64>, now: u64) -> bool {
    // A clock set back counts as passed: otherwise it would not check again until it caught up.
    let passed = |at: Option<u64>, wait: u64| at.is_none_or(|at| now < at || now - at >= wait);
    passed(answered, CHECK_EVERY_SECS) && passed(failed, RETRY_AFTER_SECS)
}

/// Seconds since 1970, by the system's clock.
pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// Checks on a thread of its own, which ends with it; the answer reaches `window` as message
/// `message` (see `take_answer`). Meanwhile nothing else waits for it.
pub fn check_in_background(window: HWND, message: u32) {
    let window = window as isize;
    std::thread::spawn(move || {
        let (wparam, lparam) = match latest() {
            Err(Unreachable) => (0, 0),
            Ok(None) => (1, 0),
            Ok(Some(version)) => (2, version.to_bits()),
        };
        // SAFETY: plain values; if the window is gone, the message is simply lost.
        unsafe { PostMessageW(window as HWND, message, wparam, lparam) };
    });
}

/// The answer that came in a message sent by `check_in_background`.
pub fn take_answer(wparam: usize, lparam: isize) -> Answer {
    match wparam {
        0 => Err(Unreachable),
        2 => Ok(Some(Version::from_bits(lparam))),
        _ => Ok(None),
    }
}

/// Asks GitHub for the latest release; it waits for the answer.
fn latest() -> Answer {
    let path = RELEASE_PAGE.strip_prefix(SITE).ok_or(Unreachable)?;
    let host = SITE.trim_start_matches("https://");
    let agent = wide(&format!("{APP_ID}/{}", Version::current()));
    // SAFETY: NUL-terminated strings that live through each call; each handle goes into a
    // `Handle`, closed when dropped (the request before the connection before the session).
    unsafe {
        let session = Handle::new(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0))?;
        WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS);
        let connection = Handle::new(WinHttpConnect(session.0, wide(host).as_ptr(), INTERNET_DEFAULT_HTTPS_PORT, 0))?;
        let request = Handle::new(WinHttpOpenRequest(
            connection.0,
            wide("HEAD").as_ptr(),
            wide(path).as_ptr(),
            null(),
            null(),
            null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        // The redirect is the answer: it is read, not followed.
        let features: u32 = WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_COOKIES;
        WinHttpSetOption(request.0, WINHTTP_OPTION_DISABLE_FEATURE, (&raw const features).cast(), 4);
        if WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0) == 0
            || WinHttpReceiveResponse(request.0, null_mut()) == 0
        {
            return Err(Unreachable);
        }
        let mut status = 0u32;
        let mut size = size_of::<u32>() as u32;
        let flags = WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER;
        if WinHttpQueryHeaders(request.0, flags, null(), (&raw mut status).cast(), &mut size, null_mut()) == 0 {
            return Err(Unreachable);
        }
        // GitHub answered: anything but a redirect means there is nothing to offer.
        if !(300..400).contains(&status) {
            return Ok(None);
        }
        let mut location = [0u16; 512];
        let mut size = size_of_val(&location) as u32;
        let read = WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_LOCATION,
            null(),
            location.as_mut_ptr().cast(),
            &mut size,
            null_mut(),
        );
        if read == 0 {
            return Ok(None);
        }
        let units = (size as usize / 2).min(location.len());
        Ok(version_in(&String::from_utf16_lossy(&location[..units])))
    }
}

/// The version of a release's page; without a stable release, `releases/latest` leads to the
/// list of releases instead, and there is none.
fn version_in(location: &str) -> Option<Version> {
    Version::parse(location.rsplit_once("/releases/tag/")?.1)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_of_a_release_page() {
        let page = "https://github.com/sbtcontreras/atk-phantom-profile-switcher/releases/tag/v1.2.10";
        assert_eq!(version_in(page), Some(Version([1, 2, 10])));
        assert_eq!(version_in("https://github.com/sbtcontreras/atk-phantom-profile-switcher/releases"), None);
        assert_eq!(version_in("https://github.com/owner/repo/releases/tag/v2.0.0-beta.1"), None);
    }

    #[test]
    fn versions_are_three_numbers() {
        assert_eq!(Version::parse("1.0.0"), Some(Version([1, 0, 0])));
        assert_eq!(Version::parse("v0.3.1"), Some(Version([0, 3, 1])));
        for wrong in ["", "1", "1.2", "1.2.3.4", "1.2.x", "1..3", "+1.2.3", "1.2.70000"] {
            assert_eq!(Version::parse(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn newer_compares_number_by_number() {
        assert!(Version([1, 10, 0]) > Version([1, 9, 9]));
        assert!(Version([2, 0, 0]) > Version([1, 99, 99]));
        assert!(!Version::current().is_newer());
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn a_version_survives_the_trip_in_a_message() {
        for version in [Version([1, 0, 0]), Version([65535, 65535, 65535]), Version([0, 0, 1])] {
            assert_eq!(take_answer(2, version.to_bits()), Ok(Some(version)));
        }
        assert_eq!(take_answer(1, 0), Ok(None));
        assert_eq!(take_answer(0, 0), Err(Unreachable));
    }

    #[test]
    fn checks_once_a_day() {
        let day = CHECK_EVERY_SECS;
        assert!(due(None, None, 1_000));
        assert!(!due(Some(1_000), None, 1_000 + day - 1));
        assert!(due(Some(1_000), None, 1_000 + day));
        // The clock was set back.
        assert!(due(Some(1_000 + day), None, 1_000));
    }

    #[test]
    fn after_a_failure_it_waits_an_hour() {
        let (day, hour) = (CHECK_EVERY_SECS, RETRY_AFTER_SECS);
        assert!(!due(None, Some(1_000), 1_000 + hour - 1));
        assert!(due(None, Some(1_000), 1_000 + hour));
        // A day since the last answer, but the try a minute ago failed.
        assert!(!due(Some(1_000), Some(1_000 + day), 1_000 + day + 60));
    }

    #[test]
    fn the_release_page_is_on_github() {
        assert!(RELEASE_PAGE.strip_prefix(SITE).is_some_and(|path| path.ends_with("/releases/latest")));
    }

    /// Asks the real GitHub: `cargo test -- --ignored`, with a connection.
    #[test]
    #[ignore = "needs a connection to GitHub"]
    fn github_answers() {
        assert!(latest().is_ok());
    }
}
