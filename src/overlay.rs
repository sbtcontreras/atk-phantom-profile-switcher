//! The on-screen notice when switching profiles: a small panel at the bottom center of the screen
//! the user is looking at, like Windows' own volume flyout, with the profile's badge, its name and
//! what it has. It fades in, waits and fades out (at once when Windows' animations are off), never
//! takes the focus or the clicks, and is drawn over games in borderless windows.

use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION, FW_NORMAL, FW_SEMIBOLD, GetDC, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY,
    MONITORINFO, MonitorFromWindow, ReleaseDC,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetForegroundWindow, HWND_TOPMOST, RegisterClassW, SPI_GETCLIENTAREAANIMATION,
    SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, ShowWindow,
    SystemParametersInfoW, ULW_ALPHA, UpdateLayeredWindow, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::APP_ID;
use crate::badge;
use crate::device::Slot;
use crate::paint::{Canvas, Dc, Font, Rect, Rgb, Selected, Text};
use crate::theme;
use crate::win::wide;

const FADE_IN: Duration = Duration::from_millis(120);
const HOLD: Duration = Duration::from_millis(1500);
const FADE_OUT: Duration = Duration::from_millis(300);
/// How often the fade advances (about 60 frames per second), only while it lasts.
const FRAME_MS: u32 = 16;

// The panel's measures, in pixels at 100% scaling.
const PADDING: f32 = 14.0;
const BADGE: f32 = 32.0;
/// Between the badge and the text.
const GAP: f32 = 12.0;
/// Between the title and the details.
const LINE_GAP: f32 = 2.0;
const RADIUS: f32 = 10.0;
/// How far the shadow reaches around the panel.
const SHADOW: f32 = 12.0;
/// How far the shadow falls below the panel.
const SHADOW_DROP: f32 = 2.0;
/// The shadow is the panel's shape this many times, each larger and as faint as this.
const SHADOW_STEPS: usize = 8;
const SHADOW_STEP_OPACITY: f32 = 0.035;
/// From the bottom of the work area (above the taskbar) to the panel.
const MARGIN: f32 = 48.0;
const TITLE_SIZE: f32 = 15.0;
const DETAILS_SIZE: f32 = 13.0;
/// The opacity of the panel's hairline border, in either theme.
const BORDER_OPACITY: f32 = 0.10;

/// The panel's colors for the dark and the light Windows theme.
struct Palette {
    panel: Rgb,
    border: Rgb,
    title: Rgb,
    details: Rgb,
}

const DARK: Palette =
    Palette { panel: Rgb(32, 32, 32), border: Rgb::WHITE, title: Rgb::WHITE, details: Rgb(200, 200, 200) };
const LIGHT: Palette =
    Palette { panel: Rgb(249, 249, 249), border: Rgb::BLACK, title: Rgb(26, 26, 26), details: Rgb(96, 96, 96) };

pub struct Overlay {
    window: HWND,
    /// When the current notice appeared, while it is on screen.
    shown_at: Option<Instant>,
    animate: bool,
}

impl Overlay {
    pub fn new() -> Option<Overlay> {
        // SAFETY: NUL-terminated strings that live through each call; the window is never shown
        // until there is something to show.
        let window = unsafe {
            let instance = GetModuleHandleW(null());
            let class = wide(&format!("{APP_ID}Overlay"));
            let window_class = WNDCLASSW {
                lpfnWndProc: Some(DefWindowProcW),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..zeroed()
            };
            RegisterClassW(&window_class);
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                instance,
                null(),
            )
        };
        (!window.is_null()).then_some(Overlay { window, shown_at: None, animate: true })
    }

    /// Shows the notice for `slot`, starting over if one is on screen. Returns how often, in
    /// milliseconds, `tick` must be called until it says the notice is gone.
    pub fn show(&mut self, slot: Slot, title: &str, details: &str) -> u32 {
        // SAFETY: the foreground window may be any; only its monitor is asked for.
        let monitor = unsafe { MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY) };
        // SAFETY: MONITORINFO is plain data with its size set; the DPI goes into plain integers.
        let (work, scale) = unsafe {
            let mut info: MONITORINFO = zeroed();
            info.cbSize = size_of::<MONITORINFO>() as u32;
            GetMonitorInfoW(monitor, &mut info);
            let (mut dpi, mut dpi_y) = (96u32, 96u32);
            GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
            (info.rcWork, dpi as f32 / 96.0)
        };
        let palette = if theme::light_shell() { &LIGHT } else { &DARK };
        let canvas = panel(slot, title, details, scale, palette);
        let (width, height) = (canvas.width as i32, canvas.height as i32);
        let x = work.left + (work.right - work.left - width) / 2;
        let y = work.bottom - (MARGIN * scale).round() as i32 - height + (SHADOW * scale).round() as i32;
        self.animate = animations_on();
        let alpha = if self.animate { 0 } else { 255 };
        if self.present(&canvas, POINT { x, y }, SIZE { cx: width, cy: height }, alpha) {
            self.shown_at = Some(Instant::now());
            // SAFETY: shows this program's window without activating it, above every other.
            unsafe {
                ShowWindow(self.window, SW_SHOWNOACTIVATE);
                SetWindowPos(self.window, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
            }
        }
        if self.animate { FRAME_MS } else { HOLD.as_millis() as u32 }
    }

    /// Advances the notice; `false` once it is gone.
    pub fn tick(&mut self) -> bool {
        let Some(shown_at) = self.shown_at else { return false };
        match opacity(shown_at.elapsed(), self.animate) {
            Some(opacity) => {
                self.fade((opacity * 255.0).round() as u8);
                true
            }
            None => {
                // SAFETY: hides this program's window.
                unsafe { ShowWindow(self.window, SW_HIDE) };
                self.shown_at = None;
                false
            }
        }
    }

    /// Puts `canvas` on screen at `position`, at `alpha`; `false` if Windows refused it.
    fn present(&self, canvas: &Canvas, position: POINT, size: SIZE, alpha: u8) -> bool {
        let Some(bitmap) = canvas.bitmap() else { return false };
        let Some(dc) = Dc::new() else { return false };
        let Some(_selected) = Selected::new(&dc, bitmap.handle()) else { return false };
        let source = POINT { x: 0, y: 0 };
        // SAFETY: the screen's context is released right after; the bitmap stays selected for
        // the call and Windows copies its pixels.
        unsafe {
            let screen = GetDC(null_mut());
            let done = UpdateLayeredWindow(
                self.window,
                screen,
                &position,
                &size,
                dc.handle(),
                &source,
                0,
                &blend(alpha),
                ULW_ALPHA,
            );
            ReleaseDC(null_mut(), screen);
            done != 0
        }
    }

    /// Changes only the notice's opacity.
    fn fade(&self, alpha: u8) {
        // SAFETY: without a source, Windows keeps the pixels and applies the new blend.
        unsafe {
            UpdateLayeredWindow(
                self.window,
                null_mut(),
                null(),
                null(),
                null_mut(),
                null(),
                0,
                &blend(alpha),
                ULW_ALPHA,
            )
        };
    }
}

fn blend(alpha: u8) -> BLENDFUNCTION {
    BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    }
}

/// The notice's opacity `elapsed` after it appeared, or `None` once it is over.
fn opacity(elapsed: Duration, animate: bool) -> Option<f32> {
    if !animate {
        return (elapsed < HOLD).then_some(1.0);
    }
    let ease = |t: f32| t * t * (3.0 - 2.0 * t);
    if elapsed < FADE_IN {
        Some(ease(elapsed.as_secs_f32() / FADE_IN.as_secs_f32()))
    } else if elapsed < FADE_IN + HOLD {
        Some(1.0)
    } else if elapsed < FADE_IN + HOLD + FADE_OUT {
        Some(1.0 - ease((elapsed - FADE_IN - HOLD).as_secs_f32() / FADE_OUT.as_secs_f32()))
    } else {
        None
    }
}

/// Whether Windows' animation effects are on (Settings › Accessibility › Visual effects).
fn animations_on() -> bool {
    let mut on = 1i32;
    // SAFETY: writes a BOOL into `on`.
    unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&raw mut on).cast(), 0) };
    on != 0
}

/// The panel, with its shadow around it.
fn panel(slot: Slot, title: &str, details: &str, scale: f32, palette: &Palette) -> Canvas {
    let px = |v: f32| (v * scale).round();
    let title = Text::new(title, Font { height: px(TITLE_SIZE) as i32, weight: FW_SEMIBOLD });
    let details = (!details.is_empty())
        .then(|| Text::new(details, Font { height: px(DETAILS_SIZE) as i32, weight: FW_NORMAL }))
        .flatten();
    let width_of = |text: &Option<Text>| text.as_ref().map_or(0.0, |t| t.width as f32);
    let height_of = |text: &Option<Text>| text.as_ref().map_or(0.0, |t| t.height as f32);
    let text_width = width_of(&title).max(width_of(&details));
    let text_height = height_of(&title) + if details.is_some() { px(LINE_GAP) + height_of(&details) } else { 0.0 };
    let (padding, badge_side, shadow) = (px(PADDING), px(BADGE), px(SHADOW));
    let content_height = badge_side.max(text_height);
    let panel_width = padding + badge_side + px(GAP) + text_width + padding;
    let panel_height = padding + content_height + padding;
    let mut canvas = Canvas::new((panel_width + 2.0 * shadow) as usize, (panel_height + 2.0 * shadow) as usize);
    let rect = Rect { x: shadow, y: shadow, width: panel_width, height: panel_height };

    // A soft shadow: the panel's shape, larger and fainter, a little below it.
    for step in (1..=SHADOW_STEPS).rev() {
        let grow = shadow * step as f32 / SHADOW_STEPS as f32;
        let around = Rect {
            x: rect.x - grow,
            y: rect.y - grow + px(SHADOW_DROP),
            width: rect.width + 2.0 * grow,
            height: rect.height + 2.0 * grow,
        };
        canvas.rounded_rect(around, px(RADIUS) + grow, Rgb::BLACK, Rgb::BLACK, SHADOW_STEP_OPACITY);
    }
    canvas.rounded_rect(rect, px(RADIUS), palette.border, palette.border, BORDER_OPACITY);
    let inner = Rect { x: rect.x + 1.0, y: rect.y + 1.0, width: rect.width - 2.0, height: rect.height - 2.0 };
    canvas.rounded_rect(inner, px(RADIUS) - 1.0, palette.panel, palette.panel, 1.0);

    let content_top = rect.y + padding;
    let badge = badge::draw(Some(slot), badge_side as usize);
    canvas.draw(&badge, (rect.x + padding) as usize, (content_top + (content_height - badge_side) / 2.0) as usize);
    let text_x = (rect.x + padding + badge_side + px(GAP)) as isize;
    let mut text_y = (content_top + (content_height - text_height) / 2.0) as isize;
    if let Some(title) = &title {
        canvas.text(title, text_x, text_y, palette.title);
        text_y += (title.height as f32 + px(LINE_GAP)) as isize;
    }
    if let Some(details) = &details {
        canvas.text(details, text_x, text_y, palette.details);
    }
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fades_in_holds_and_fades_out() {
        assert_eq!(opacity(Duration::ZERO, true), Some(0.0));
        assert_eq!(opacity(FADE_IN + HOLD / 2, true), Some(1.0));
        let leaving = opacity(FADE_IN + HOLD + FADE_OUT / 2, true).unwrap_or(0.0);
        assert!(leaving > 0.0 && leaving < 1.0);
        assert_eq!(opacity(FADE_IN + HOLD + FADE_OUT, true), None);
    }

    #[test]
    fn without_animations_it_shows_at_once_and_leaves_at_once() {
        assert_eq!(opacity(Duration::ZERO, false), Some(1.0));
        assert_eq!(opacity(HOLD, false), None);
    }

    #[test]
    fn the_panel_grows_with_its_text_and_the_scale() {
        let short = panel(Slot::ALL[0], "Profile 1", "", 1.0, &DARK);
        let long = panel(Slot::ALL[0], "Profile 1", "4000 Hz · 1600 DPI · competitive sensor", 1.0, &DARK);
        let scaled = panel(Slot::ALL[0], "Profile 1", "", 2.0, &DARK);
        assert!(long.width > short.width);
        assert!(scaled.width > short.width * 3 / 2);
    }
}
