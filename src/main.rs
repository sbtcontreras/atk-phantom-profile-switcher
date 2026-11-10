//! Phantom Switch: cambia la configuración interna (onboard) del ATK Phantom Ultimate desde un
//! ícono junto al reloj de Windows, con los mismos comandos que ATK HUB.
//!
//! - `device`: el mouse (su protocolo, aparte de la E/S de Windows, para probarlo).
//! - `worker`: el hilo que le habla al mouse; la interfaz nunca espera al mouse.
//! - `app`: la interfaz y lo que se sabe del mouse; `tray`, `menu` e `icon` la dibujan.
//!
//! No escucha el mouse ni el teclado: le habla solo cuando se le pide, cuando Windows avisa que
//! se conectó o desconectó algo y, por el receptor, para la batería cada 15 minutos.

#![windows_subsystem = "windows"]

mod app;
mod device;
mod icon;
mod menu;
mod startup;
mod strings;
mod theme;
mod tray;
mod win;
mod worker;

fn main() {
    let Some(_instance) = win::SingleInstance::acquire("PhantomSwitch") else {
        win::message(strings::ALREADY_RUNNING);
        return;
    };
    theme::follow_system();
    app::run();
}
