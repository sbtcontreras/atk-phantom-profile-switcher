//! Drawing in memory: an RGBA canvas with antialiased rounded rectangles and system-font text,
//! turned into whatever Windows needs (a tray icon, a menu bitmap, a layered window's pixels).
//! The tray badge, the menu badges and the on-screen notice are all drawn with it.

use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLEARTYPE_NATURAL_QUALITY, CLIP_DEFAULT_PRECIS, CreateBitmap,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CALCRECT,
    DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_TOP, DeleteDC, DeleteObject, DrawTextW, GdiFlush, HBITMAP, HDC, HGDIOBJ,
    OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO};

use crate::win::wide;

/// An sRGB color.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const WHITE: Rgb = Rgb(255, 255, 255);

    /// This color moved `amount` (0 to 1) of the way to white.
    pub fn lighten(self, amount: f32) -> Rgb {
        self.mix(Rgb::WHITE, amount)
    }

    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let m = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Rgb(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2))
    }

    fn linear(self) -> [f32; 3] {
        [self.0, self.1, self.2].map(|c| f32::from(c) / 255.0)
    }
}

/// A rectangle in canvas pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A system font: Segoe UI at a pixel height and weight.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Font {
    pub height: i32,
    pub weight: u32,
}

/// Text rendered once, ready to be measured and drawn.
pub struct Text {
    pub width: usize,
    pub height: usize,
    /// Coverage, 0 to 255, row by row.
    ink: Vec<u8>,
}

impl Text {
    /// `text` drawn white on black with the font's antialiasing, kept as coverage.
    pub fn new(text: &str, font: Font) -> Option<Text> {
        let text = wide(text);
        let units = text.len() as i32 - 1;
        // SAFETY: every GDI object sits in a guard; guards drop in reverse order, so each object is
        // deselected before it is deleted and the context goes last.
        unsafe {
            let dc = Dc::new()?;
            let font = Object(CreateFontW(
                -font.height,
                0,
                0,
                0,
                font.weight as i32,
                0,
                0,
                0,
                u32::from(DEFAULT_CHARSET),
                u32::from(OUT_DEFAULT_PRECIS),
                u32::from(CLIP_DEFAULT_PRECIS),
                CLEARTYPE_NATURAL_QUALITY,
                u32::from(DEFAULT_PITCH),
                wide("Segoe UI").as_ptr(),
            ));
            let _font = Selected::new(&dc, font.0)?;
            // Measured as it is drawn: natural widths run wider than GDI's own measure says.
            let mut measured = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            DrawTextW(dc.handle(), text.as_ptr(), units, &mut measured, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
            // A little room on the right: antialiasing reaches past the last advance.
            let (width, height) = (measured.right.max(1) as usize + 2, measured.bottom.max(1) as usize);
            let (bitmap, bits) = dib(width, height)?;
            let bitmap = Object(bitmap);
            let _bitmap = Selected::new(&dc, bitmap.0)?;
            SetBkMode(dc.handle(), TRANSPARENT as i32);
            SetTextColor(dc.handle(), 0x00FF_FFFF);
            let mut rect = RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 };
            DrawTextW(dc.handle(), text.as_ptr(), units, &mut rect, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);
            GdiFlush();
            let pixels = std::slice::from_raw_parts(bits, width * height * 4);
            // ClearType's natural widths space letters well; its three subpixel coverages,
            // averaged, are the grayscale coverage a layered or alpha image needs.
            let ink = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|[b, g, r, _]| ((u16::from(*b) + u16::from(*g) + u16::from(*r)) / 3) as u8)
                .collect();
            Some(Text { width, height, ink })
        }
    }

    /// The box of the pixels the text actually covers (a font's line has room above and below).
    pub fn inked(&self) -> Option<(usize, usize, usize, usize)> {
        let lit = || (0..self.ink.len()).filter(|i| self.ink[*i] > 32);
        let left = lit().map(|i| i % self.width).min()?;
        let right = lit().map(|i| i % self.width).max()?;
        let top = lit().map(|i| i / self.width).min()?;
        let bottom = lit().map(|i| i / self.width).max()?;
        Some((left, top, right, bottom))
    }
}

/// Straight-alpha RGBA pixels, 0 to 1, top to bottom.
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pixels: Vec<[f32; 4]>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Canvas {
        Canvas { width, height, pixels: vec![[0.0; 4]; width * height] }
    }

    /// A rounded rectangle filled with a vertical gradient from `top` to `bottom`, at `opacity`.
    pub fn rounded_rect(&mut self, rect: Rect, radius: f32, top: Rgb, bottom: Rgb, opacity: f32) {
        let (x0, x1) = (rect.x.floor().max(0.0) as usize, ((rect.x + rect.width).ceil() as usize).min(self.width));
        let (y0, y1) = (rect.y.floor().max(0.0) as usize, ((rect.y + rect.height).ceil() as usize).min(self.height));
        let (cx, cy) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        for y in y0..y1 {
            let along = ((y as f32 + 0.5 - rect.y) / rect.height).clamp(0.0, 1.0);
            let color = top.mix(bottom, along);
            for x in x0..x1 {
                let coverage =
                    rounded_rect_coverage(x as f32 - cx, y as f32 - cy, rect.width / 2.0, rect.height / 2.0, radius);
                if coverage > 0.0 {
                    self.blend(x, y, color.linear(), coverage * opacity);
                }
            }
        }
    }

    /// `text` in `color`, its top left corner at (`x`, `y`).
    pub fn text(&mut self, text: &Text, x: isize, y: isize, color: Rgb) {
        for ty in 0..text.height {
            for tx in 0..text.width {
                let ink = text.ink[ty * text.width + tx];
                let (px, py) = (x + tx as isize, y + ty as isize);
                if ink > 0 && (0..self.width as isize).contains(&px) && (0..self.height as isize).contains(&py) {
                    self.blend(px as usize, py as usize, color.linear(), f32::from(ink) / 255.0);
                }
            }
        }
    }

    /// Another canvas over this one, its top left corner at (`x`, `y`).
    pub fn draw(&mut self, other: &Canvas, x: usize, y: usize) {
        for oy in 0..other.height.min(self.height.saturating_sub(y)) {
            for ox in 0..other.width.min(self.width.saturating_sub(x)) {
                let [r, g, b, a] = other.pixels[oy * other.width + ox];
                if a > 0.0 {
                    self.blend(x + ox, y + oy, [r, g, b], a);
                }
            }
        }
    }

    /// `color` over the pixel at `alpha`, the usual "over" of straight alpha.
    fn blend(&mut self, x: usize, y: usize, color: [f32; 3], alpha: f32) {
        let pixel = &mut self.pixels[y * self.width + x];
        let out = alpha + pixel[3] * (1.0 - alpha);
        if out <= 0.0 {
            return;
        }
        for i in 0..3 {
            pixel[i] = (color[i] * alpha + pixel[i] * pixel[3] * (1.0 - alpha)) / out;
        }
        pixel[3] = out;
    }

    /// BGRA bytes, top to bottom; `premultiplied` for menus and layered windows, which expect it.
    pub fn bgra(&self, premultiplied: bool) -> Vec<u8> {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        self.pixels
            .iter()
            .flat_map(|[r, g, b, a]| {
                let k = if premultiplied { *a } else { 1.0 };
                [byte(b * k), byte(g * k), byte(r * k), byte(*a)]
            })
            .collect()
    }

    pub fn icon(&self) -> Option<Icon> {
        let (color, bits) = dib(self.width, self.height)?;
        let color = Object(color);
        let pixels = self.bgra(false);
        // SAFETY: `bits` has room for every pixel; Windows copies both bitmaps into the icon.
        unsafe {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits, pixels.len());
            // With alpha, Windows ignores the mask but still wants one: rows aligned to 16 bits.
            let mask_bits = vec![0u8; self.width.div_ceil(16) * 2 * self.height];
            let mask = Object(CreateBitmap(self.width as i32, self.height as i32, 1, 1, mask_bits.as_ptr().cast()));
            let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask.0, hbmColor: color.0 };
            let icon = CreateIconIndirect(&info);
            (!icon.is_null()).then_some(Icon(icon))
        }
    }

    /// A premultiplied 32-bit bitmap, as menus and layered windows take it.
    pub fn bitmap(&self) -> Option<Bitmap> {
        let (bitmap, bits) = dib(self.width, self.height)?;
        let bitmap = Bitmap(bitmap);
        let pixels = self.bgra(true);
        // SAFETY: `bits` has room for every pixel.
        unsafe { std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits, pixels.len()) };
        Some(bitmap)
    }
}

/// How much of a pixel at (`x`, `y`) from the center a rounded rectangle covers (4 × 4 samples).
fn rounded_rect_coverage(x: f32, y: f32, half_width: f32, half_height: f32, radius: f32) -> f32 {
    let mut inside = 0;
    for sy in 0..4 {
        for sx in 0..4 {
            let px = x + (sx as f32 + 0.5) / 4.0;
            let py = y + (sy as f32 + 0.5) / 4.0;
            let qx = px.abs() - (half_width - radius);
            let qy = py.abs() - (half_height - radius);
            if qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius <= 0.0 {
                inside += 1;
            }
        }
    }
    inside as f32 / 16.0
}

/// A Windows icon, destroyed when dropped.
pub struct Icon(HICON);

impl Icon {
    pub fn handle(&self) -> HICON {
        self.0
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: an icon created by `Canvas::icon`, owned by nobody else.
        unsafe { DestroyIcon(self.0) };
    }
}

/// A GDI bitmap, deleted when dropped.
pub struct Bitmap(HBITMAP);

impl Bitmap {
    pub fn handle(&self) -> HBITMAP {
        self.0
    }
}

impl Drop for Bitmap {
    fn drop(&mut self) {
        // SAFETY: a bitmap created by `Canvas::bitmap`; whoever used it no longer selects it.
        unsafe { DeleteObject(self.0) };
    }
}

/// A 32-bit top-down DIB and its pixels; the caller puts the bitmap in a guard.
fn dib(width: usize, height: usize) -> Option<(HBITMAP, *mut u8)> {
    // SAFETY: BITMAPINFO is plain data.
    unsafe {
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..zeroed()
        };
        let mut bits = null_mut();
        let bitmap: HBITMAP = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        (!bitmap.is_null() && !bits.is_null()).then(|| (bitmap, bits.cast::<u8>()))
    }
}

/// A GDI object, deleted when dropped.
struct Object(HGDIOBJ);

impl Drop for Object {
    fn drop(&mut self) {
        // SAFETY: an object created here and already deselected (`Selected` guards drop first).
        unsafe { DeleteObject(self.0) };
    }
}

/// A memory device context, deleted when dropped.
pub struct Dc(HDC);

impl Dc {
    pub fn new() -> Option<Dc> {
        // SAFETY: a memory context compatible with the screen.
        let dc = unsafe { CreateCompatibleDC(null_mut()) };
        (!dc.is_null()).then_some(Dc(dc))
    }

    pub fn handle(&self) -> HDC {
        self.0
    }
}

impl Drop for Dc {
    fn drop(&mut self) {
        // SAFETY: a context created by `CreateCompatibleDC`.
        unsafe { DeleteDC(self.0) };
    }
}

/// An object selected into a context; dropping it selects back what was there.
pub struct Selected {
    dc: HDC,
    previous: HGDIOBJ,
}

impl Selected {
    pub fn new(dc: &Dc, object: HGDIOBJ) -> Option<Selected> {
        if object.is_null() {
            return None;
        }
        // SAFETY: a valid context and object.
        let previous = unsafe { SelectObject(dc.0, object) };
        Some(Selected { dc: dc.0, previous })
    }
}

impl Drop for Selected {
    fn drop(&mut self) {
        // SAFETY: gives the context back what it had.
        unsafe { SelectObject(self.dc, self.previous) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rounded_rect_is_solid_inside_and_round_at_the_corners() {
        assert_eq!(rounded_rect_coverage(0.0, 0.0, 16.0, 16.0, 9.0), 1.0);
        assert_eq!(rounded_rect_coverage(-16.0, 0.0, 16.0, 16.0, 9.0), 1.0);
        assert_eq!(rounded_rect_coverage(-16.0, -16.0, 16.0, 16.0, 9.0), 0.0);
    }

    #[test]
    fn blending_over_keeps_straight_alpha() {
        let mut canvas = Canvas::new(1, 1);
        canvas.blend(0, 0, [1.0, 0.0, 0.0], 0.5);
        canvas.blend(0, 0, [0.0, 0.0, 1.0], 0.5);
        let [r, _, b, a] = canvas.pixels[0];
        assert!((a - 0.75).abs() < 1e-6);
        assert!((r - 1.0 / 3.0).abs() < 1e-6 && (b - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn premultiplied_bytes_carry_the_alpha() {
        let mut canvas = Canvas::new(1, 1);
        canvas.blend(0, 0, [1.0, 1.0, 1.0], 0.5);
        assert_eq!(canvas.bgra(true), [128, 128, 128, 128]);
        assert_eq!(canvas.bgra(false), [255, 255, 255, 128]);
    }
}
