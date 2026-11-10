<div align="center">

# Phantom Switch

**Tus configuraciones del ATK Phantom, a un clic del reloj.**

[![Versión](https://img.shields.io/github/v/release/sbtcontreras/atk-phantom-profile-switcher?include_prereleases&label=versi%C3%B3n&color=6366f1)](https://github.com/sbtcontreras/atk-phantom-profile-switcher/releases)
[![Windows 10 y 11](https://img.shields.io/badge/Windows-10%20%C2%B7%2011-0078d4)](#compatibilidad)
[![Licencia MIT](https://img.shields.io/badge/licencia-MIT-22c55e)](LICENSE)

*[In English below](#in-english)*

</div>

---

El ATK Phantom Ultimate guarda cuatro configuraciones en su memoria —DPI, polling rate, modo del
sensor, botones—, pero cambiar de una a otra exige abrir ATK HUB en el navegador. **Phantom Switch**
vive junto al reloj de Windows y lo hace en un clic: pasa de tu configuración normal a la
competitiva y vuelve, sin abrir nada, sin instalar nada y sin tocar el rendimiento del mouse.

## Lo que hace

- **Un clic para cambiar.** El ícono muestra el número de la configuración activa, cada una con su
  color, y un clic te devuelve a la que usaste antes.
- **Todo a la vista.** El menú muestra cómo está conectado el mouse, su batería y, para cada
  configuración, el polling rate, el DPI y el modo del sensor, leídos del propio mouse.
- **Avisa antes de quedarte sin batería.** Una notificación al bajar del 15 % y otra al 5 %.
- **Se ve en casa en Windows.** Íconos nítidos con cualquier escalado de pantalla, menús en modo
  oscuro cuando Windows lo está, y manejo con el teclado.
- **Arranca con Windows**, si quieres, y tiene a mano un acceso a ATK HUB.

## Descarga

Descarga **`PhantomSwitch.exe`** desde [Releases](https://github.com/sbtcontreras/atk-phantom-profile-switcher/releases)
y ábrelo. Es un solo archivo de menos de 400 KB: no se instala ni deja archivos al lado.

> La primera vez, Windows puede mostrar «Windows protegió su PC», porque el programa no está
> firmado. Elige **Más información › Ejecutar de todas formas**. Cada versión la compila GitHub
> desde este mismo código, y junto a cada `.exe` se publica su SHA-256.

## Cómo se usa

Arma tus configuraciones en [ATK HUB](https://hub.atk.pro/) como siempre. Phantom Switch se encarga
de cambiar entre ellas.

| | |
|---|---|
| **Clic** en el ícono | Vuelve a la configuración que usaste antes; la primera vez, alterna entre la 1 y la 2. |
| **Clic derecho** | Abre el menú: conexión y batería, las cuatro configuraciones con lo que tiene cada una, «Abrir ATK HUB», «Iniciar con Windows» y Salir. |
| **Teclado** | <kbd>Win</kbd> + <kbd>B</kbd> lleva a la bandeja; <kbd>Enter</kbd> cambia y la tecla de menú abre el menú. |

El menú lee el mouse al abrirse:

```text
Inalámbrico · batería 90 %
────────────────────────────────────────────────────────────
  Configuración 1      1000 Hz · 1600 DPI · sensor básico
● Configuración 2      4000 Hz · 1600 DPI · sensor competitivo
  Configuración 3
  Configuración 4
────────────────────────────────────────────────────────────
  Abrir ATK HUB
  Iniciar con Windows
  Salir
```

La configuración activa se lee en ese momento; las demás muestran lo que tenían la última vez que
estuvieron activas.

## Ligero por diseño

Phantom Switch está hecho para no notarse.

- **No escucha el mouse ni el teclado.** No se mete entre el mouse y tus juegos: a 8000 Hz, cada
  movimiento llega a Windows como si el programa no existiera.
- **En reposo no hace nada.** Cero uso de procesador y unos 2 MB de memoria.
- **Le habla al mouse solo cuando hace falta:** al usar el ícono, al conectar o desconectar algo por
  USB y, por el receptor, para revisar la batería cada 15 minutos.
- **Nunca toca tus ajustes.** Lee, y su única escritura es la que hace ATK HUB al cambiar de
  configuración. Envía cada pedido una sola vez, nunca dos cambios a la vez, y no reescribe la
  configuración que ya está activa.

> Usa ATK HUB o Phantom Switch, no los dos a la vez: el receptor atiende un pedido por vez y se
> perderían respuestas.

## Compatibilidad

- **ATK Phantom Ultimate** con firmware BITMOUSE, por cable o con su receptor 8K. Es el modelo
  probado; otros mouse de ATK con el mismo firmware podrían funcionar a través de su receptor.
- **Windows 10 (1903 o posterior) y Windows 11.** Sin permisos de administrador.

## Cómo funciona

Phantom Switch habla con el mouse por HID, con los mismos pedidos que la página de ATK HUB, leídos
de su código público. El protocolo está documentado en
[`src/device/protocol.rs`](src/device/protocol.rs) y probado con respuestas reales del mouse:

| Comando | Para qué |
|---:|---|
| `9` | Configuración activa y su polling rate |
| `10` | Activar una configuración |
| `7` · `15` | Batería · si está cargando |
| `23` | Leer la memoria de la configuración activa: tabla de DPI y modo del sensor |
| `177` | Funciones del mouse |

El código está en Rust, sin dependencias más allá de las APIs de Windows:

```text
src/
├── device/      el mouse: su protocolo (sin E/S, probado) y la E/S HID de Windows
├── worker.rs    el hilo que le habla al mouse: la interfaz nunca lo espera
├── app.rs       la interfaz y lo que se sabe del mouse
├── tray.rs      el ícono junto al reloj
├── menu.rs      su menú
├── icon.rs      los íconos, dibujados al tamaño que pide Windows
└── …            textos, tema oscuro, inicio con Windows
```

## Compilar

Con [Rust](https://rustup.rs) en Windows:

```bash
cargo build --release
```

Desde Linux o WSL, con [`cargo-xwin`](https://github.com/rust-cross/cargo-xwin):
`cargo xwin build --release`. El ejecutable queda en `target/x86_64-pc-windows-msvc/release/`.

Cada etiqueta `vX.Y.Z` publica su versión sola: GitHub Actions corre las pruebas, compila y la sube a
Releases.

## Licencia

[MIT](LICENSE). Phantom Switch es un proyecto independiente, sin relación con ATK.

---

## In English

**Phantom Switch** switches the onboard profiles of the **ATK Phantom Ultimate** mouse from the
Windows tray, without opening ATK HUB. Click the icon to go back to your previous profile;
right-click for the battery, the connection and each profile's polling rate, DPI and sensor mode,
read from the mouse itself. It warns you before the battery runs out, follows Windows' dark mode
and scaling, and can start with Windows.

It is a single ~400 KB executable with no installer. It never hooks mouse or keyboard input, uses no
CPU at rest, and besides reads it only sends the same "activate profile" packet ATK HUB sends. The
interface is in Spanish. The HID protocol is documented in
[`src/device/protocol.rs`](src/device/protocol.rs). Not affiliated with ATK.
