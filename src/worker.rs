//! El hilo que habla con el mouse. Es el único que lo toca: recibe pedidos de la interfaz, los
//! atiende de a uno y le devuelve cada resultado como un mensaje a su ventana.

use std::sync::mpsc::{Receiver, Sender, channel};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::device::{Battery, Connection, Disconnected, Phantom, Slot, Snapshot};

/// Lo que la interfaz le pide al mouse.
#[derive(Clone, Copy, Debug)]
pub enum Request {
    /// Leer lo que tiene.
    Refresh,
    /// Leer lo que tiene para abrir el menú.
    Menu,
    /// Volver a la configuración usada antes de la activa.
    Back {
        previous: Option<Slot>,
    },
    Activate(Slot),
    /// Solo la batería, y solo por el receptor: sin el mouse ya encontrado, no hace nada.
    Battery,
}

/// Lo que vuelve de un pedido.
#[derive(Debug)]
pub struct Report {
    pub request: Request,
    pub outcome: Outcome,
}

#[derive(Debug)]
pub enum Outcome {
    /// No hay ningún Phantom conectado (o se desconectó).
    NotFound,
    /// Está conectado, pero no contestó.
    NoAnswer,
    /// Lo que se leyó; con un cambio pedido, `switched` dice si el mouse lo confirmó.
    Read {
        connection: Connection,
        snapshot: Snapshot,
        switched: Option<bool>,
    },
    Battery(Battery),
}

/// El extremo del hilo que usa la interfaz.
pub struct Worker {
    requests: Sender<Request>,
}

impl Worker {
    /// Arranca el hilo; sus resultados llegan a `window` como el mensaje `message`, con un
    /// `Box<Report>` en el `lParam` (ver `take_report`).
    pub fn spawn(window: HWND, message: u32) -> Worker {
        let (requests, receiver) = channel();
        let window = window as isize;
        std::thread::spawn(move || run(&receiver, window, message));
        Worker { requests }
    }

    pub fn send(&self, request: Request) {
        // El hilo vive tanto como el programa: un envío no falla mientras haya ventana.
        let _ = self.requests.send(request);
    }
}

/// El `Report` que llegó en el `lParam` del mensaje de resultados.
///
/// # Safety
/// `lparam` debe ser el de un mensaje enviado por este hilo, y tomarse una sola vez.
pub unsafe fn take_report(lparam: isize) -> Box<Report> {
    // SAFETY: según el contrato, es un `Box<Report>` que se entregó con `Box::into_raw`.
    unsafe { Box::from_raw(lparam as *mut Report) }
}

fn run(receiver: &Receiver<Request>, window: isize, message: u32) {
    let mut mouse: Option<Phantom> = None;
    for request in receiver {
        let Some(outcome) = serve(&mut mouse, request) else { continue };
        let report = Box::into_raw(Box::new(Report { request, outcome }));
        // SAFETY: la ventana vive mientras corre el programa; si el mensaje no se entrega, el
        // `Report` se recupera aquí y se suelta.
        if unsafe { PostMessageW(window as HWND, message, 0, report as isize) } == 0 {
            // SAFETY: `report` viene de `Box::into_raw` y nadie más lo recibió.
            drop(unsafe { Box::from_raw(report) });
        }
    }
}

/// Atiende un pedido; `None` cuando no hay nada que contar (la batería sin receptor).
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
    match read(phantom, request) {
        Ok(outcome) => Some(outcome),
        Err(Disconnected) => {
            *mouse = None;
            Some(Outcome::NotFound)
        }
    }
}

fn read(phantom: &mut Phantom, request: Request) -> crate::device::Result<Outcome> {
    let target = match request {
        Request::Refresh | Request::Menu | Request::Battery => None,
        // Ya activa: no se le escribe al mouse lo que ya tiene.
        Request::Activate(slot) => (phantom.active_slot()? != Some(slot)).then_some(slot),
        Request::Back { previous } => phantom.active_slot()?.map(|active| crate::app::back_from(active, previous)),
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
