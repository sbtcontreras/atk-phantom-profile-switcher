# ATK Phantom Modo

Cambia la configuración interna (onboard) del **ATK Phantom Ultimate** desde un ícono junto al
reloj de Windows, sin abrir ATK HUB. Un clic para pasar de tu configuración normal a la
competitiva, y un menú con la batería y el polling rate, el DPI y el modo del sensor de cada una.

*In English: [below](#in-english).*

> Programa independiente, sin relación con ATK. Usa los mismos comandos que la página oficial
> [ATK HUB](https://hub.atk.pro/), leídos de su código público. Probado con el ATK Phantom
> Ultimate de firmware BITMOUSE, por cable y con su receptor 8K.

## Descarga

En [Releases](../../releases/latest), `AtkPhantomModo.exe`: un solo archivo de unos 270 KB, sin
instalación ni archivos al lado. Funciona en Windows 10 y 11.

Windows puede avisar «Windows protegió su PC» la primera vez, porque el programa no está firmado:
**Más información › Ejecutar de todas formas**.

## Uso

El ícono muestra el número de la configuración activa (1 azul, 2 rojo, 3 verde, 4 ámbar), o **?**
gris si no encuentra el mouse.

- **Clic**: vuelve a la configuración que usaste antes (la 1 o la 2 mientras no hayas usado otra).
- **Clic derecho**: el menú, leído del mouse al abrirlo:

  ```
  Inalámbrico · batería 90 %
  ──────────────────────────────────────────────────────────
    Configuración 1    1000 Hz · 1600 DPI · sensor básico
  ✓ Configuración 2    4000 Hz · 1600 DPI · sensor competitivo
    Configuración 3
    Configuración 4
  ──────────────────────────────────────────────────────────
    Abrir ATK HUB
    Iniciar con Windows
    Salir
  ```

  La activa se lee en ese momento; las otras muestran lo que se leyó la última vez que estuvieron
  activas.
- **Batería baja**: por el receptor, revisa la batería cada 15 minutos y avisa con una
  notificación al bajar del 15 % y del 5 %. Mientras juegas a pantalla completa, Windows guarda
  las notificaciones en su centro de notificaciones.

Las configuraciones se arman en ATK HUB (DPI, polling rate, sensor, botones…); este programa solo
cambia de una a otra. No uses la página y el programa a la vez: el receptor atiende un pedido a la
vez y se pierden respuestas.

## Qué le hace al mouse

- **No escucha el mouse ni el teclado.** No está en el camino de lo que el mouse le manda a
  Windows: en reposo no usa procesador.
- **Solo lee**, salvo un comando: activar otra configuración (1 a 4). Nunca escribe DPI, botones,
  macros ni firmware.
- **Envía cada paquete una vez**, nunca dos cambios a la vez, y no escribe la configuración que el
  mouse ya tiene.
- Le habla cuando se lo pides, cuando Windows avisa que se conectó o desconectó algo por USB y,
  por el receptor, para la batería cada 15 minutos.

### El protocolo

Interfaz HID `0xFF05` / uso 1 (VID `0x373B`), reporte 8, paquetes de 63 bytes:

```
[checksum] [0x72] [largo de parámetros] [secuencia] [destino] [comando] [largo] [datos…]
```

El checksum es la suma de los demás bytes. El destino es 0 para el mouse por cable (PID 4814 o
4882) y 1 para hablarle a través de su receptor. La respuesta repite el comando en el byte 4 y trae
sus datos desde el 6.

| Comando | Qué hace |
|---|---|
| 9 | Configuración activa (0 a 3), polling rate |
| 10 | Activa una configuración |
| 7 / 15 | Batería / si está cargando |
| 23 | Lee la memoria de la configuración activa: tabla de DPI en la dirección 1, modo del sensor en la 74 |
| 177 | Funciones del mouse (dónde está el modo del sensor) |

`diagnostico.js` se pega en la consola del navegador (F12) con hub.atk.pro abierta y el mouse
conectado ahí: solo lee, con los mismos comandos, y muestra lo que el mouse responde, para
compararlo con la página o probar otro mouse ATK.

## Compilar

Con Rust (canal estable) en Windows:

```bash
cargo build --release
```

O desde Linux/WSL con [`cargo-xwin`](https://github.com/rust-cross/cargo-xwin):
`cargo xwin build --release`. El `.exe` queda en `target/x86_64-pc-windows-msvc/release/`.

Cada versión se compila y publica sola en GitHub Actions al subir su etiqueta (`v0.1.0`…), desde
este mismo código.

## Licencia

[MIT](LICENSE).

---

## In English

A tiny Windows tray app (Rust, ~270 KB, no install) to switch the onboard profiles of the
**ATK Phantom Ultimate** mouse without opening ATK HUB: click the tray icon to go back to the
previous profile, right-click for a menu with the battery, connection type and each profile's
polling rate, DPI and sensor mode. It warns when the battery drops below 15 % and 5 % (receiver
only). It does not hook mouse or keyboard input and uses no CPU at rest; besides reads, the only
command it sends is "activate profile N", the same packet ATK HUB sends. The interface is in
Spanish. Not affiliated with ATK; tested with the BITMOUSE-firmware Phantom Ultimate, wired and
with its 8K dongle. The HID protocol is documented above.
