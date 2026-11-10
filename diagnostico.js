// Diagnóstico para pegar en la consola de hub.atk.pro (F12 › Consola), con el mouse conectado
// en la página. Solo LEE (no cambia nada): configuración activa, polling rate, DPI, modo del
// sensor y batería, con los mismos comandos que usa ATK HUB. Al final copia el resultado al
// portapapeles.
(async () => {
  const hex = (v) => [...new Uint8Array(v.buffer ?? v)].map((b) => b.toString(16).padStart(2, "0")).join(" ");
  const RATES = { 0: 1000, 1: 500, 2: 250, 3: 125, 4: 8000, 5: 4000, 6: 2000 };
  const WIRED = [4814, 4882];

  const device = (await navigator.hid.getDevices()).find(
    (d) => [0x373b, 0x3554].includes(d.vendorId) && d.collections.some((c) => c.usagePage === 0xff05 && c.usage === 1),
  );
  if (!device) return console.warn("No veo el Phantom: conéctalo en ATK HUB primero.");
  if (!device.opened) await device.open();
  const target = WIRED.includes(device.productId) ? 0 : 1;

  // Paquete BITMOUSE: checksum, 0x72, largo de parámetros, secuencia, destino, comando, largo, datos…
  const packet = (command, paramLen, cmdLen, data = []) => {
    const p = new Uint8Array(63);
    p[1] = 114;
    p[2] = paramLen;
    p[4] = target;
    p[5] = command;
    p[6] = cmdLen;
    p.set(data, 7);
    p[0] = p.slice(1).reduce((a, b) => a + b, 0) & 255;
    return p;
  };
  // La respuesta a ese comando (byte 4), sin el aviso de espera del receptor.
  const ask = (p) =>
    new Promise((resolve) => {
      const on = (e) => {
        const r = new Uint8Array(e.data.buffer);
        if (r[4] !== p[5]) return;
        device.removeEventListener("inputreport", on);
        clearTimeout(timer);
        resolve(r);
      };
      const timer = setTimeout(() => {
        device.removeEventListener("inputreport", on);
        resolve(null);
      }, 1500);
      device.addEventListener("inputreport", on);
      device.sendReport(8, p);
    });
  const at = (r, k) => (r ? r[6 + k] : null);
  const readAddress = async (address, len) => {
    const r = await ask(packet(23, 10, 13, [address & 255, address >> 8, len]));
    return r ? [...r.slice(9, 9 + len)] : null;
  };

  const out = { pid: device.productId, target, raw: {} };

  const config = await ask(packet(9, 20, 18));
  out.raw.config = config && hex(config);
  out.configuracion = at(config, 0) + 1;
  out.pollingCodigo = at(config, 2);
  out.pollingHz = RATES[at(config, 2)] ?? "desconocido";
  out.nivelDpiEnConfig = at(config, 3);

  const level = await ask(packet(7, 2, 1));
  const charging = await ask(packet(15, 2, 1));
  out.raw.bateria = level && hex(level);
  out.raw.carga = charging && hex(charging);
  out.bateria = at(level, 0);
  out.cargando = at(charging, 0) === 1;

  // La tabla de DPI completa, como la lee ATK HUB: 7 lecturas de 10 bytes desde la dirección 1.
  const table = [];
  for (let s = 0; s < 7; s++) table.push(...((await readAddress(s * 10 + 1, 10)) ?? []));
  out.raw.tablaDpi = hex(table);
  const view = new DataView(Uint8Array.from(table).buffer);
  if (table.length >= 66) {
    out.nivelDpiActivo = view.getUint8(0);
    out.nivelesDpi = view.getUint8(1);
    out.dpi = Array.from({ length: 8 }, (_, i) => `${view.getUint16(2 + i * 8, true)}×${view.getUint16(4 + i * 8, true)}`);
  }

  // Modo del sensor («Sensor Sampling Rate»), como lo lee ATK HUB: según el bit 8 de las
  // funciones del mouse (comando 177), con el comando 37 en modo lectura (operación 0) o en la
  // dirección 74.
  const features = await ask(packet(177, 3, 1));
  out.raw.funciones = features && hex(features);
  out.funciones = at(features, 0);
  if (features && (at(features, 0) & 8) === 8) {
    const performance = await ask(packet(37, 5, 3, [0]));
    out.raw.sensor = performance && hex(performance);
    const mode = at(performance, 1);
    const corded = at(performance, 2);
    out.sensor = mode === 2 ? (corded === 0 ? "MAX 13K" : "MAX 20K") : mode === 1 ? "Competitivo" : "Básico";
  } else {
    const model = await readAddress(74, 1);
    out.raw.sensor = model && hex(model);
    out.sensor = { 0: "Básico", 4: "Competitivo", 5: "MAX" }[model?.[0]] ?? "desconocido";
  }

  // Lo mismo en 2 lecturas: el nivel activo y solo su DPI.
  const head = await readAddress(1, 2);
  const entry = head && (await readAddress(3 + head[0] * 8, 4));
  out.atajo = entry && { nivel: head[0], dpi: `${entry[0] | (entry[1] << 8)}×${entry[2] | (entry[3] << 8)}` };

  const text = JSON.stringify(out, null, 2);
  console.log(text);
  try {
    copy(text);
    console.log("Copiado al portapapeles.");
  } catch {
    console.log("Copia el texto de arriba.");
  }
})();
