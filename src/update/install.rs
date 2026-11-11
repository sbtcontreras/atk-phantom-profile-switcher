//! Installing a newer release in place of this one, when the user chooses to.
//!
//! The release's `.exe` and its signature are downloaded, and the `.exe` goes in place only if
//! the signature is Phantom Switch's (`signature`). Windows does not let a running `.exe` be
//! overwritten, but it lets it be renamed: this one becomes `PhantomSwitch.exe.old` and the new
//! one takes its name, so **Start with Windows** keeps starting it. The new one starts with
//! `--updated <pid>`, waits for this one to end and deletes the `.old`.

use std::io;
use std::mem::zeroed;
use std::path::{Path, PathBuf};
use std::ptr::null;
use std::{env, fs};

use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, OpenProcess, PROCESS_INFORMATION, PROCESS_SYNCHRONIZE, STARTUPINFOW, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::{Unreachable, Version, http, signature};
use crate::win::wide;

/// The file each release publishes, beside its signature (`.sig`).
const ASSET: &str = "PhantomSwitch.exe";
const SIGNATURE_LIMIT: usize = 1024;
/// Far above the `.exe`'s ~400 KB: anything larger is not it.
const ASSET_LIMIT: usize = 32 << 20;
/// How the new copy is started: followed by the process id of the one it replaces.
const UPDATED_FLAG: &str = "--updated";
/// How long the new copy waits for the one it replaces to close.
const WAIT_MS: u32 = 30_000;

/// How an install ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// In place and started: this copy must close.
    Restarting,
    /// In place, but the new copy did not start: it will the next time the app opens.
    Installed,
    /// GitHub could not be reached: nothing changed.
    Unreachable,
    /// The download is not signed with Phantom Switch's key: nothing changed.
    NotAuthentic,
    /// It cannot update itself here (a build without the key, a release without a signature, a
    /// folder it cannot write to): the release's page is for downloading it by hand.
    ByHand,
}

impl Outcome {
    const ALL: [Outcome; 5] =
        [Outcome::Restarting, Outcome::Installed, Outcome::Unreachable, Outcome::NotAuthentic, Outcome::ByHand];
}

/// Whether this build can install an update itself; otherwise the menu leads to its page.
pub fn can_install() -> bool {
    signature::has_key()
}

/// Installs `version` on a thread of its own, which ends with it; the outcome reaches `window` as
/// message `message` (see `take_outcome`).
pub fn in_background(window: HWND, message: u32, version: Version) {
    let window = window as isize;
    std::thread::spawn(move || {
        let outcome = install(version);
        // SAFETY: plain values; if the window is gone, the message is simply lost.
        unsafe { PostMessageW(window as HWND, message, outcome as usize, 0) };
    });
}

/// The outcome that came in a message sent by `in_background`.
pub fn take_outcome(wparam: usize) -> Outcome {
    Outcome::ALL.get(wparam).copied().unwrap_or(Outcome::ByHand)
}

/// At start, before taking the single instance: after an update, waits for the copy that
/// installed it to close. Whether this start follows an update.
pub fn wait_for_previous() -> bool {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some(UPDATED_FLAG) {
        return false;
    }
    if let Some(pid) = args.next().and_then(|pid| pid.parse().ok()) {
        // SAFETY: a handle only to wait on the process, closed after it.
        unsafe {
            let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if !process.is_null() {
                WaitForSingleObject(process, WAIT_MS);
                CloseHandle(process);
            }
        }
    }
    true
}

/// At start, as the only copy running: deletes what an update left beside the `.exe`.
pub fn clean_up() {
    if let Ok(exe) = env::current_exe() {
        for leftover in [old(&exe), new(&exe)] {
            let _ = fs::remove_file(leftover);
        }
    }
}

fn install(version: Version) -> Outcome {
    let Ok(exe) = env::current_exe() else { return Outcome::ByHand };
    if !can_install() {
        return Outcome::ByHand;
    }
    let url = format!("{}/releases/download/v{version}/{ASSET}", env!("CARGO_PKG_REPOSITORY"));
    let signature = match http::download(&format!("{url}.sig"), SIGNATURE_LIMIT) {
        Ok(Some(signature)) => signature,
        Ok(None) => return Outcome::ByHand,
        Err(Unreachable) => return Outcome::Unreachable,
    };
    let data = match http::download(&url, ASSET_LIMIT) {
        Ok(Some(data)) => data,
        Ok(None) => return Outcome::ByHand,
        Err(Unreachable) => return Outcome::Unreachable,
    };
    if !signature::is_authentic(&data, &signature) {
        return Outcome::NotAuthentic;
    }
    if replace(&exe, &data).is_err() {
        return Outcome::ByHand;
    }
    if start(&exe) { Outcome::Restarting } else { Outcome::Installed }
}

/// Starts `exe` as the copy that replaces this one. Straight through Windows: `std::process`
/// would weigh a sixth of the `.exe`.
fn start(exe: &Path) -> bool {
    let application = wide(&exe.to_string_lossy());
    let mut command = wide(&format!("\"{}\" {UPDATED_FLAG} {}", exe.display(), std::process::id()));
    // SAFETY: NUL-terminated strings that live through the call, the command line writable as
    // Windows wants it; the new process's handles are closed right away.
    unsafe {
        let startup = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, ..zeroed() };
        let mut process: PROCESS_INFORMATION = zeroed();
        let started = CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            0,
            0,
            null(),
            null(),
            &startup,
            &mut process,
        ) != 0;
        if started {
            CloseHandle(process.hProcess);
            CloseHandle(process.hThread);
        }
        started
    }
}

/// Puts `data` in place of the running `exe`, which becomes the `.old`; if a step fails, it
/// leaves things as they were.
fn replace(exe: &Path, data: &[u8]) -> io::Result<()> {
    let (new, old) = (new(exe), old(exe));
    fs::write(&new, data)?;
    // One an earlier update left, if it is still there.
    let _ = fs::remove_file(&old);
    let swapped = rename(exe, &old).and_then(|()| {
        rename(&new, exe).inspect_err(|_| {
            let _ = rename(&old, exe);
        })
    });
    if swapped.is_err() {
        let _ = fs::remove_file(&new);
    }
    swapped
}

fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = (wide(&from.to_string_lossy()), wide(&to.to_string_lossy()));
    // SAFETY: NUL-terminated paths that live through the call.
    let moved = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
    if moved == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

fn new(exe: &Path) -> PathBuf {
    beside(exe, ".new")
}

fn old(exe: &Path) -> PathBuf {
    beside(exe, ".old")
}

/// `exe` with `suffix` after its whole name: `PhantomSwitch.exe.old`.
fn beside(exe: &Path, suffix: &str) -> PathBuf {
    let mut path = exe.as_os_str().to_owned();
    path.push(suffix);
    path.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outcome_survives_the_trip_in_a_message() {
        for outcome in Outcome::ALL {
            assert_eq!(take_outcome(outcome as usize), outcome);
        }
        assert_eq!(take_outcome(99), Outcome::ByHand);
    }

    #[test]
    fn leftovers_go_beside_the_exe() {
        let exe = Path::new(r"C:\Tools\PhantomSwitch.exe");
        assert_eq!(old(exe), Path::new(r"C:\Tools\PhantomSwitch.exe.old"));
        assert_eq!(new(exe), Path::new(r"C:\Tools\PhantomSwitch.exe.new"));
    }

    #[test]
    fn replaces_a_file_keeping_the_old_one_beside_it() -> io::Result<()> {
        let dir = env::temp_dir().join(format!("phantom-switch-test-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let exe = dir.join("PhantomSwitch.exe");
        fs::write(&exe, b"old")?;
        fs::write(old(&exe), b"older")?;
        replace(&exe, b"new")?;
        assert_eq!(fs::read(&exe)?, b"new");
        assert_eq!(fs::read(old(&exe))?, b"old");
        assert!(!new(&exe).exists());
        fs::remove_dir_all(&dir)
    }

    #[test]
    fn the_release_files_are_on_github() {
        let url = format!("{}/releases/download/v1.2.3/{ASSET}", env!("CARGO_PKG_REPOSITORY"));
        assert!(url.starts_with(http::SITE));
    }

    /// Downloads a real release: `cargo test -- --ignored`, with a connection.
    #[test]
    #[ignore = "needs a connection to GitHub"]
    fn github_has_the_release() {
        let url = format!("{}/releases/download/v1.0.0/{ASSET}", env!("CARGO_PKG_REPOSITORY"));
        assert!(matches!(http::download(&url, ASSET_LIMIT), Ok(Some(data)) if data.starts_with(b"MZ")));
    }
}
