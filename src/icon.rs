//! Los íconos de la bandeja: un cuadrado redondeado con un degradado del color de la
//! configuración y su número en blanco (Segoe UI semibold), dibujados al tamaño exacto que pide
//! Windows, así que se ven nítidos con cualquier escalado de pantalla.

use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLIP_DEFAULT_PRECIS, CreateBitmap, CreateCompatibleDC,
    CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, FW_SEMIBOLD, GdiFlush, HBITMAP, HDC, HGDIOBJ,
    OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO};

use crate::win::wide;

/// Un color sRGB.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Un ícono de Windows, destruido al soltarse.
pub struct Icon(HICON);

impl Icon {
    pub fn handle(&self) -> HICON {
        self.0
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: ícono creado por `badge` y de nadie más.
        unsafe { DestroyIcon(self.0) };
    }
}

/// Cuánto se aclara el borde de arriba del degradado.
const GRADIENT_LIFT: f32 = 0.18;
/// Radio de las esquinas, en proporción al lado.
const CORNER: f32 = 0.28;
/// Alto de la letra, en proporción al lado.
const TEXT_HEIGHT: f32 = 0.74;

/// El ícono de `label` sobre `color`, de `size` × `size` píxeles.
pub fn badge(label: &str, color: Rgb, size: i32) -> Option<Icon> {
    let size = usize::try_from(size.clamp(16, 256)).ok()?;
    from_pixels(&badge_pixels(label, color, size)?, size)
}

/// Los píxeles BGRA de `badge`, de arriba abajo.
fn badge_pixels(label: &str, color: Rgb, size: usize) -> Option<Vec<u8>> {
    let ink = centered(&text_ink(label, size)?, size);
    let top = lift(color, GRADIENT_LIFT);
    let mut pixels = vec![0u8; size * size * 4];
    for y in 0..size {
        let background = mix(top, color, y as f32 / (size - 1) as f32);
        for x in 0..size {
            let i = y * size + x;
            let Rgb(r, g, b) = mix(background, Rgb(255, 255, 255), f32::from(ink[i]) / 255.0);
            let alpha = (rounded_square_coverage(x, y, size) * 255.0).round() as u8;
            pixels[i * 4..i * 4 + 4].copy_from_slice(&[b, g, r, alpha]);
        }
    }
    Some(pixels)
}

/// Cuánto de un píxel cubre el cuadrado redondeado (4 × 4 muestras).
fn rounded_square_coverage(x: usize, y: usize, size: usize) -> f32 {
    let half = size as f32 / 2.0;
    let radius = size as f32 * CORNER;
    let mut inside = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            let px = x as f32 + (sx as f32 + 0.5) / 4.0 - half;
            let py = y as f32 + (sy as f32 + 0.5) / 4.0 - half;
            let qx = px.abs() - (half - radius);
            let qy = py.abs() - (half - radius);
            let distance = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
            if distance <= 0.0 {
                inside += 1;
            }
        }
    }
    inside as f32 / 16.0
}

fn lift(Rgb(r, g, b): Rgb, amount: f32) -> Rgb {
    let up = |c: u8| (f32::from(c) + (255.0 - f32::from(c)) * amount).round() as u8;
    Rgb(up(r), up(g), up(b))
}

fn mix(Rgb(r1, g1, b1): Rgb, Rgb(r2, g2, b2): Rgb, t: f32) -> Rgb {
    let m = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Rgb(m(r1, r2), m(g1, g2), m(b1, b2))
}

/// La tinta movida para que su contorno quede al centro: las cifras de una fuente no están
/// centradas en su línea.
fn centered(ink: &[u8], size: usize) -> Vec<u8> {
    let lit = || (0..size * size).filter(|i| ink[*i] > 32);
    let (Some(left), Some(right)) = (lit().map(|i| i % size).min(), lit().map(|i| i % size).max()) else {
        return ink.to_vec();
    };
    let (top, bottom) = (lit().map(|i| i / size).min().unwrap_or(0), lit().map(|i| i / size).max().unwrap_or(0));
    let dx = ((size - 1) as isize - (left + right) as isize) / 2;
    let dy = ((size - 1) as isize - (top + bottom) as isize) / 2;
    let mut moved = vec![0u8; size * size];
    for y in 0..size {
        for x in 0..size {
            let (sx, sy) = (x as isize - dx, y as isize - dy);
            if (0..size as isize).contains(&sx) && (0..size as isize).contains(&sy) {
                moved[y * size + x] = ink[sy as usize * size + sx as usize];
            }
        }
    }
    moved
}

/// La tinta (0 a 255) de `label` en blanco sobre negro, con el suavizado de la fuente.
fn text_ink(label: &str, size: usize) -> Option<Vec<u8>> {
    let side = size as i32;
    // SAFETY: cada objeto de GDI queda en un guardián; se sueltan en orden inverso, así que cada
    // uno se deselecciona antes de borrarse y el contexto se borra al final.
    unsafe {
        let dc = Dc(CreateCompatibleDC(null_mut()));
        if dc.0.is_null() {
            return None;
        }
        let (bitmap, bits) = dib(size)?;
        let _bitmap = Selected::new(&dc, bitmap.0)?;
        let font = Object(CreateFontW(
            -((size as f32 * TEXT_HEIGHT).round() as i32),
            0,
            0,
            0,
            FW_SEMIBOLD as i32,
            0,
            0,
            0,
            u32::from(DEFAULT_CHARSET),
            u32::from(OUT_DEFAULT_PRECIS),
            u32::from(CLIP_DEFAULT_PRECIS),
            u32::from(ANTIALIASED_QUALITY),
            u32::from(DEFAULT_PITCH),
            wide("Segoe UI").as_ptr(),
        ));
        let _font = Selected::new(&dc, font.0)?;
        SetBkMode(dc.0, TRANSPARENT as i32);
        SetTextColor(dc.0, 0x00FF_FFFF);
        let mut rect = RECT { left: 0, top: 0, right: side, bottom: side };
        DrawTextW(dc.0, wide(label).as_ptr(), -1, &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        GdiFlush();
        let pixels = std::slice::from_raw_parts(bits, size * size * 4);
        Some(pixels.as_chunks::<4>().0.iter().map(|[b, g, r, _]| (*b).max(*g).max(*r)).collect())
    }
}

/// Un ícono con canal alfa a partir de sus píxeles BGRA, de arriba abajo.
fn from_pixels(pixels: &[u8], size: usize) -> Option<Icon> {
    let (color, bits) = dib(size)?;
    // SAFETY: `bits` tiene `size`² píxeles de 4 bytes; los mapas de bits se copian en el ícono.
    unsafe {
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits, pixels.len());
        // Con alfa, Windows no usa la máscara, pero la pide: filas alineadas a 16 bits.
        let mask_bits = vec![0u8; size.div_ceil(16) * 2 * size];
        let mask = Object(CreateBitmap(size as i32, size as i32, 1, 1, mask_bits.as_ptr().cast()));
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask.0, hbmColor: color.0 };
        let icon = CreateIconIndirect(&info);
        (!icon.is_null()).then_some(Icon(icon))
    }
}

/// Un mapa de bits de 32 bits, de arriba abajo, y sus píxeles.
fn dib(size: usize) -> Option<(Object, *mut u8)> {
    // SAFETY: BITMAPINFO es datos planos; el mapa de bits queda en un guardián.
    unsafe {
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..zeroed()
        };
        let mut bits = null_mut();
        let bitmap: HBITMAP = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        (!bitmap.is_null() && !bits.is_null()).then(|| (Object(bitmap), bits.cast::<u8>()))
    }
}

/// Un objeto de GDI, borrado al soltarse.
struct Object(HGDIOBJ);

impl Drop for Object {
    fn drop(&mut self) {
        // SAFETY: objeto creado aquí y ya deseleccionado (los `Selected` se sueltan antes).
        unsafe { DeleteObject(self.0) };
    }
}

struct Dc(HDC);

impl Drop for Dc {
    fn drop(&mut self) {
        // SAFETY: contexto creado por `CreateCompatibleDC`.
        unsafe { DeleteDC(self.0) };
    }
}

/// Un objeto seleccionado en un contexto: al soltarse, vuelve a seleccionar el anterior.
struct Selected {
    dc: HDC,
    previous: HGDIOBJ,
}

impl Selected {
    fn new(dc: &Dc, object: HGDIOBJ) -> Option<Selected> {
        if object.is_null() {
            return None;
        }
        // SAFETY: contexto y objeto válidos.
        let previous = unsafe { SelectObject(dc.0, object) };
        Some(Selected { dc: dc.0, previous })
    }
}

impl Drop for Selected {
    fn drop(&mut self) {
        // SAFETY: devuelve al contexto lo que tenía antes.
        unsafe { SelectObject(self.dc, self.previous) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_square_is_solid_inside_and_round_at_the_corners() {
        assert_eq!(rounded_square_coverage(16, 16, 32), 1.0);
        assert_eq!(rounded_square_coverage(0, 16, 32), 1.0);
        assert_eq!(rounded_square_coverage(0, 0, 32), 0.0);
    }

    #[test]
    fn centers_the_ink() {
        let mut ink = vec![0u8; 16];
        ink[0] = 255; // arriba a la izquierda, en 4 × 4
        let moved = centered(&ink, 4);
        assert_eq!(moved.iter().position(|v| *v == 255), Some(5));
    }
}
