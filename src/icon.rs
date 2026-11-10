//! El ícono de la bandeja: un círculo de color con una cifra blanca, dibujado a mano.

use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS, DeleteObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

const SIZE: usize = 32;
const SCALE: usize = 3;

/// Cifras de 5 × 7 puntos; cualquier otra cosa, un signo de pregunta.
fn glyph(label: char) -> [&'static str; 7] {
    match label {
        '1' => ["00100", "01100", "00100", "00100", "00100", "00100", "01110"],
        '2' => ["01110", "10001", "00001", "00010", "00100", "01000", "11111"],
        '3' => ["11110", "00001", "00001", "01110", "00001", "00001", "11110"],
        '4' => ["00010", "00110", "01010", "10010", "11111", "00010", "00010"],
        _ => ["01110", "10001", "00001", "00010", "00100", "00000", "00100"],
    }
}

pub fn make(label: char, (r, g, b): (u8, u8, u8)) -> HICON {
    // BGRA de arriba abajo.
    let mut pixels = vec![0u8; SIZE * SIZE * 4];
    let center = (SIZE as f32 - 1.0) / 2.0;
    let radius = SIZE as f32 / 2.0 - 0.5;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let distance = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            let i = (y * SIZE + x) * 4;
            pixels[i..i + 4].copy_from_slice(&[b, g, r, (coverage * 255.0) as u8]);
        }
    }
    let (left, top) = ((SIZE - 5 * SCALE) / 2 + 1, (SIZE - 7 * SCALE) / 2);
    for (row, bits) in glyph(label).iter().enumerate() {
        for (column, bit) in bits.bytes().enumerate() {
            if bit != b'1' {
                continue;
            }
            for dy in 0..SCALE {
                for dx in 0..SCALE {
                    let i = ((top + row * SCALE + dy) * SIZE + left + column * SCALE + dx) * 4;
                    pixels[i..i + 3].copy_from_slice(&[255, 255, 255]);
                }
            }
        }
    }

    // SAFETY: los bitmaps se copian en el ícono y se borran enseguida; `bits` tiene SIZE² píxeles.
    unsafe {
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: SIZE as i32,
            biHeight: -(SIZE as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..zeroed()
        };
        let mut bits = null_mut();
        let color = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if color.is_null() {
            return null_mut();
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), pixels.len());
        let mask_bits = [0u8; SIZE * SIZE / 8];
        let mask = CreateBitmap(SIZE as i32, SIZE as i32, 1, 1, mask_bits.as_ptr().cast());
        let icon = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let handle = CreateIconIndirect(&icon);
        DeleteObject(color);
        DeleteObject(mask);
        handle
    }
}
