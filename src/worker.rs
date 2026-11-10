//! The thread that talks to the mouse. It is the only one that touches it: it takes jobs from the
//! interface, does them one at a time and sends each reply back as a message to the interface's
//! window.

use std::sync::mpsc::{Receiver, Sender, channel};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::device::{self, Battery, Connection, Disconnected, Phantom, Slot, Snapshot};

/// What the interface asks of the mouse.
#[derive(Clone, Copy, Debug)]
pub enum Job {
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

impl Job {
    pub fn switches(self) -> bool {
        matches!(self, Job::Back { .. } | Job::Activate(_))
    }
}

/// What comes back from a job.
#[derive(Debug)]
pub struct Reply {
    pub job: Job,
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
    jobs: Sender<Job>,
}

impl Worker {
    /// Starts the thread; its replies reach `window` as message `message`, with a `Box<Reply>` in
    /// `lParam` (see `take_reply`).
    pub fn spawn(window: HWND, message: u32) -> Worker {
        let (jobs, receiver) = channel();
        let window = window as isize;
        std::thread::spawn(move || run(&receiver, window, message));
        Worker { jobs }
    }

    pub fn send(&self, job: Job) {
        // The thread lives as long as the program: a send cannot fail while there is a window.
        let _ = self.jobs.send(job);
    }
}

/// The `Reply` that came in the `lParam` of the replies message.
///
/// # Safety
/// `lparam` must be that of a message sent by this thread, and be taken only once.
pub unsafe fn take_reply(lparam: isize) -> Box<Reply> {
    // SAFETY: by contract, a `Box<Reply>` handed over with `Box::into_raw`.
    unsafe { Box::from_raw(lparam as *mut Reply) }
}

fn run(receiver: &Receiver<Job>, window: isize, message: u32) {
    let mut mouse: Option<Phantom> = None;
    for job in receiver {
        let outcome = match job {
            Job::Battery => battery(&mut mouse),
            Job::Refresh | Job::Menu | Job::Back { .. } | Job::Activate(_) => Some(read(&mut mouse, job)),
        };
        let Some(outcome) = outcome else { continue };
        let reply = Box::into_raw(Box::new(Reply { job, outcome }));
        // SAFETY: the window lives while the program runs; if the message is not delivered, the
        // `Reply` is taken back here and dropped.
        if unsafe { PostMessageW(window as HWND, message, 0, reply as isize) } == 0 {
            // SAFETY: `reply` comes from `Box::into_raw` and nobody else received it.
            drop(unsafe { Box::from_raw(reply) });
        }
    }
}

/// The battery, through the receiver of a mouse already found; `None` when there is nothing to
/// tell.
fn battery(mouse: &mut Option<Phantom>) -> Option<Outcome> {
    let phantom = mouse.as_mut().filter(|m| m.connection() == Connection::Wireless)?;
    match phantom.battery() {
        Ok(battery) => battery.map(Outcome::Battery),
        Err(Disconnected) => {
            *mouse = None;
            Some(Outcome::NotFound)
        }
    }
}

/// Finds the mouse if needed, switches if `job` asks for it, then reads what the mouse has.
fn read(mouse: &mut Option<Phantom>, job: Job) -> Outcome {
    if mouse.is_none() {
        *mouse = Phantom::find();
    }
    let Some(phantom) = mouse.as_mut() else { return Outcome::NotFound };
    match switch_and_read(phantom, job) {
        Ok(outcome) => outcome,
        Err(Disconnected) => {
            *mouse = None;
            Outcome::NotFound
        }
    }
}

fn switch_and_read(phantom: &mut Phantom, job: Job) -> device::Result<Outcome> {
    let target = match job {
        // Already active: the mouse is not written what it already has.
        Job::Activate(slot) => (phantom.active_slot()? != Some(slot)).then_some(slot),
        Job::Back { previous } => phantom.active_slot()?.map(|active| back_from(active, previous)),
        Job::Refresh | Job::Menu | Job::Battery => None,
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
