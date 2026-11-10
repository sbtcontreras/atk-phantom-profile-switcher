//! The badge that stands for a profile wherever it shows (the tray, the menu, the on-screen
//! notice): its number in white Segoe UI on a rounded square with a gradient of its color.

use windows_sys::Win32::Graphics::Gdi::FW_SEMIBOLD;

use crate::device::Slot;
use crate::paint::{Canvas, Font, Rect, Rgb, Text};

/// Each profile's color, and the color of not knowing which one is active.
const COLORS: [Rgb; 4] = [Rgb(37, 99, 235), Rgb(220, 38, 38), Rgb(22, 163, 74), Rgb(217, 119, 6)];
const UNKNOWN: Rgb = Rgb(107, 114, 128);

/// How much lighter the top of the gradient is.
const GRADIENT_LIFT: f32 = 0.18;
/// Corner radius and text height, as a share of the side.
const CORNER: f32 = 0.28;
const TEXT_HEIGHT: f32 = 0.74;

/// The badge of `slot` (a question mark when it is not known), `size` × `size` pixels.
pub fn draw(slot: Option<Slot>, size: usize) -> Canvas {
    let (label, color) = match slot {
        Some(slot) => (slot.number().to_string(), COLORS[slot.index()]),
        None => ("?".to_owned(), UNKNOWN),
    };
    let side = size as f32;
    let mut canvas = Canvas::new(size, size);
    let square = Rect { x: 0.0, y: 0.0, width: side, height: side };
    canvas.rounded_rect(square, side * CORNER, color.lighten(GRADIENT_LIFT), color, 1.0);
    let font = Font { height: (side * TEXT_HEIGHT).round() as i32, weight: FW_SEMIBOLD };
    if let Some(text) = Text::new(&label, font)
        && let Some((left, top, right, bottom)) = text.inked()
    {
        // Centered by what the digit covers: a font's digits do not sit in the middle of its line.
        let x = (size as isize - (right - left + 1) as isize) / 2 - left as isize;
        let y = (size as isize - (bottom - top + 1) as isize) / 2 - top as isize;
        canvas.text(&text, x, y, Rgb::WHITE);
    }
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_badge_is_opaque_inside_and_round_at_the_corners() {
        let canvas = draw(Slot::new(2), 32);
        let pixels = canvas.bgra(false);
        let alpha = |x: usize, y: usize| pixels[(y * 32 + x) * 4 + 3];
        assert_eq!(alpha(16, 2), 255);
        assert_eq!(alpha(0, 0), 0);
    }

    #[test]
    fn the_number_is_drawn_in_white() {
        let pixels = draw(Slot::new(1), 32).bgra(false);
        let whitest =
            pixels.as_chunks::<4>().0.iter().map(|[b, g, r, _]| u16::from(*b) + u16::from(*g) + u16::from(*r)).max();
        assert_eq!(whitest, Some(765));
    }
}
