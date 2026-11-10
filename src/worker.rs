//! The thread that talks to the mouse. It is the only one that touches it: it takes requests
//! from the interface, serves them one at a time and sends each result back as a message to the
//! interface's window.

use std::sync::mpsc::{Receiver, Sender, channel};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::device::{Battery, Connection, Disconnected, Phantom, Slot, Snapshot};

/// What the interface asks of the mouse.
#[derive(Clone, Copy, Debug)]
pub enum Request {
    /// Read what it has.
    Refresh,
    /// Read what it has, to open the menu.
    Menu,
    /// Go back to the profile used before the active one.
    Back {
        previous: Option<Slot>,
    },
    Activate(Slot),
    /// Only the battery, and only through the receiver: without the mouse already found, nothing.
    Battery,
}

/// What comes back from a request.
#[derive(Debug)]
pub struct Report {
    pub request: Request,
    pub outcome: Outcome,
}

#[derive(Debug)]
pub enum Outcome {
    /// No Phantom is connected (or it was disconnected).
    NotFound,
    /// It is connected, but it did not answer.
    NoAnswer,
    /// What was read; after a switch, `switched` says whether the mouse confirmed it.
    Read {
        connection: Connection,
        snapshot: Snapshot,
        switched: Option<bool>,
    },
    Battery(Battery),
}

/// The interface's end of the thread.
pub struct Worker {
    requests: Sender<Request>,
}

impl Worker {
    /// Starts the thread; its results reach `window` as message `message`, with a `Box<Report>`
    /// in `lParam` (see `take_report`).
    pub fn spawn(window: HWND, message: u32) -> Worker {
        let (requests, receiver) = channel();
        let window = window as isize;
        std::thread::spawn(move || run(&receiver, window, message));
        Worker { requests }
    }

    pub fn send(&self, request: Request) {
        // The thread lives as long as the program: a send cannot fail while there is a window.
        let _ = self.requests.send(request);
    }
}

/// The `Report` that came in the `lParam` of the results message.
///
/// # Safety
/// `lparam` must be that of a message sent by this thread, and be taken only once.
pub unsafe fn take_report(lparam: isize) -> Box<Report> {
    // SAFETY: by contract, a `Box<Report>` handed over with `Box::into_raw`.
    unsafe { Box::from_raw(lparam as *mut Report) }
}

fn run(receiver: &Receiver<Request>, window: isize, message: u32) {
    let mut mouse: Option<Phantom> = None;
    for request in receiver {
        let Some(outcome) = serve(&mut mouse, request) else { continue };
        let report = Box::into_raw(Box::new(Report { request, outcome }));
        // SAFETY: the window lives while the program runs; if the message is not delivered, the
        // `Report` is taken back here and dropped.
        if unsafe { PostMessageW(window as HWND, message, 0, report as isize) } == 0 {
            // SAFETY: `report` comes from `Box::into_raw` and nobody else received it.
            drop(unsafe { Box::from_raw(report) });
        }
    }
}

/// Serves a request; `None` when there is nothing to tell (the battery without the receiver).
fn serve(mouse: &mut Option<Phantom>, request: Request) -> Option<Outcome> {
    if let Request::Battery = request {
        let phantom = mouse.as_mut().filter(|m| m.connection() == Connection::Wireless)?;
        return match phantom.battery() {
            Ok(battery) => battery.map(Outcome::Battery),
            Err(Disconnected) => {
                *mouse = None;
                Some(Outcome::NotFound)
            }
        };
    }
    if mouse.is_none() {
        *mouse = Phantom::find();
    }
    let Some(phantom) = mouse.as_mut() else { return Some(Outcome::NotFound) };
    match carry_out(phantom, request) {
        Ok(outcome) => Some(outcome),
        Err(Disconnected) => {
            *mouse = None;
            Some(Outcome::NotFound)
        }
    }
}

/// Switches if the request asks for it, then reads what the mouse has.
fn carry_out(phantom: &mut Phantom, request: Request) -> crate::device::Result<Outcome> {
    let target = match request {
        Request::Refresh | Request::Menu | Request::Battery => None,
        // Already active: the mouse is not written what it already has.
        Request::Activate(slot) => (phantom.active_slot()? != Some(slot)).then_some(slot),
        Request::Back { previous } => phantom.active_slot()?.map(|active| back_from(active, previous)),
    };
    let switched = match target {
        Some(slot) => Some(phantom.activate(slot)?),
        None => None,
    };
    Ok(match phantom.snapshot()? {
        Some(snapshot) => Outcome::Read { connection: phantom.connection(), snapshot, switched },
        None => Outcome::NoAnswer,
    })
}

/// Where the click goes from `active`: the one used before, or the 1 or the 2 while there is none.
pub fn back_from(active: Slot, previous: Option<Slot>) -> Slot {
    let [first, second, ..] = Slot::ALL;
    previous.filter(|p| *p != active).unwrap_or(if active == first { second } else { first })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(number: u8) -> Slot {
        Slot::ALL[usize::from(number - 1)]
    }

    #[test]
    fn the_click_goes_back_to_the_one_used_before() {
        assert_eq!(back_from(slot(2), Some(slot(1))), slot(1));
        assert_eq!(back_from(slot(1), Some(slot(3))), slot(3));
        assert_eq!(back_from(slot(3), Some(slot(2))), slot(2));
    }

    #[test]
    fn without_one_before_it_alternates_the_first_two() {
        assert_eq!(back_from(slot(1), None), slot(2));
        assert_eq!(back_from(slot(2), None), slot(1));
        assert_eq!(back_from(slot(4), None), slot(1));
        assert_eq!(back_from(slot(2), Some(slot(2))), slot(1));
    }
}
