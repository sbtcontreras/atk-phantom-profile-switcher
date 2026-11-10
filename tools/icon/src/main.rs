//! Draws Phantom Switch's icon, the one source of its drawing: a fierce ghost shaped like a mouse,
//! in the ATK Phantom's own look (a bone-white shell in two panels with the mouse's rounded front,
//! a gold seam and wheel, angular eyes, a hem of spikes) on a rounded square of warm dark with a
//! light from above and a thin gold edge.
//!
//! `cargo run -p icon` writes:
//!
//! - `assets/icon.ico`, the `.exe`'s icon, a bitmap per size (small sizes leave out the details);
//! - `assets/icon.svg`, the same drawing in vectors, for the README;
//! - the desktop icon of `assets/preview.svg`, between its `<!-- icon -->` marks.
//!
//! Coordinates go from 0 to 1 across the icon, top to bottom.

use std::error::Error;
use std::fs;
use std::path::Path;

/// The sides of the `.exe`'s icon: from file lists to Explorer's large icons.
const SIZES: [usize; 8] = [16, 20, 24, 32, 40, 48, 64, 128];
/// Below this side the edge, the seam, the wheel and the shadow would be a smudge: the ghost keeps
/// only its eyes.
const DETAILS_FROM: usize = 32;
/// Below this side the wheel's knurl would be a smudge: the wheel stays plain gold.
const KNURL_FROM: usize = 64;
/// Samples per pixel along each axis.
const SAMPLES: usize = 4;

// The colors: the mouse's bone-white shell and gold details, in warm dark light.
const BONE: [f32; 3] = [0.95, 0.92, 0.86];
/// The right half of the shell, a shade darker, as a second panel catching less light.
const BONE_SHADE: [f32; 3] = [0.86, 0.82, 0.74];
const GOLD: [f32; 3] = [0.8, 0.64, 0.36];
/// The knurl's ridges.
const GOLD_DARK: [f32; 3] = [0.55, 0.42, 0.22];
const EYES: [f32; 3] = [0.09, 0.075, 0.06];
/// The background, from its top to its bottom.
const BACK_TOP: [f32; 3] = [0.2, 0.18, 0.15];
const BACK_BOTTOM: [f32; 3] = [0.075, 0.065, 0.055];
const LIGHT: [f32; 3] = [0.45, 0.39, 0.29];

// The rounded square.
const SQUARE_HALF: f32 = 0.47;
const SQUARE_RADIUS: f32 = 0.22;
/// How much the gold edge tints the background under it.
const EDGE_TINT: f32 = 0.6;
/// The gold edge's width, once a pixel is narrower.
const EDGE: f32 = 0.012;

// The light from above, as in the room the mouse sits in.
/// Its center, above the icon.
const LIGHT_CENTER: (f32, f32) = (0.5, -0.05);
/// How much flatter than wide it is.
const LIGHT_FLATTEN: f32 = 1.2;
/// How far from its center it fades out.
const LIGHT_REACH: f32 = 0.8;
/// How much it tints the background at its center.
const LIGHT_STRENGTH: f32 = 0.45;

// The soft shadow under the ghost, as if it floated.
const SHADOW_CENTER: (f32, f32) = (0.5, 0.875);
const SHADOW_RADII: (f32, f32) = (0.22, 0.03);
/// How much it darkens what is under it at its center.
const SHADOW_DARKNESS: f32 = 0.45;

// The ghost: the mouse's round front down to `ARCH`, sides that widen like a cape down to `HEM`,
// and three spikes, the middle one longest.
const ARCH: f32 = 0.45;
/// The front's radius: half the width at `ARCH`.
const ARCH_HALF: f32 = 0.22;
const HEM: f32 = 0.69;
/// Half the width at `HEM`.
const HEM_HALF: f32 = 0.27;
/// The middle spike's length below `HEM`.
const SPIKE: f32 = 0.16;
/// The outer spikes' length, as a share of the middle one's.
const OUTER_SPIKE: f32 = 0.8;

// The seam between the buttons.
/// Its half width, once a pixel is narrower.
const SEAM_HALF: f32 = 0.009;
/// Where it ends, going down from the top of the front.
const SEAM_END: f32 = 0.4;

// The wheel, knurled from its top.
const WHEEL_CENTER: (f32, f32) = (0.5, 0.3);
const WHEEL_HALF: (f32, f32) = (0.024, 0.05);
const WHEEL_RADIUS: f32 = 0.012;
/// The height of each ridge, and of each gap between ridges.
const KNURL: f32 = 0.013;

/// The left eye: an angular blade slanting down toward the middle, like the cuts in the mouse's
/// shell, its corners clockwise. The right eye is its mirror.
const EYE: [(f32, f32); 4] = [(0.355, 0.445), (0.47, 0.495), (0.462, 0.528), (0.372, 0.5)];

// What it writes, from the repository's root.
const ICO: &str = "assets/icon.ico";
const SVG: &str = "assets/icon.svg";
const PREVIEW: &str = "assets/preview.svg";
/// The side the SVG is drawn at.
const SVG_SIDE: f32 = 256.0;

fn main() -> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::write(root.join(ICO), ico())?;
    fs::write(root.join(SVG), svg())?;
    let preview = fs::read_to_string(root.join(PREVIEW))?;
    fs::write(root.join(PREVIEW), with_icon(&preview)?)?;
    Ok(())
}

/// An .ico with a 32-bit bitmap per size.
fn ico() -> Vec<u8> {
    let images: Vec<Vec<u8>> = SIZES.iter().map(|size| bitmap(*size, &pixels(*size))).collect();
    let mut file = Vec::new();
    file.extend_from_slice(&[0, 0, 1, 0]);
    file.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len();
    for (size, image) in SIZES.iter().zip(&images) {
        let side = *size as u8;
        file.extend_from_slice(&[side, side, 0, 0]);
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&32u16.to_le_bytes());
        file.extend_from_slice(&(image.len() as u32).to_le_bytes());
        file.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    for image in images {
        file.extend_from_slice(&image);
    }
    file
}

/// An .ico image: its header, BGRA pixels bottom to top and an empty mask.
fn bitmap(size: usize, rgba: &[[f32; 4]]) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4;
    let mut image = Vec::new();
    image.extend_from_slice(&40u32.to_le_bytes());
    image.extend_from_slice(&(size as i32).to_le_bytes());
    image.extend_from_slice(&(2 * size as i32).to_le_bytes());
    image.extend_from_slice(&1u16.to_le_bytes());
    image.extend_from_slice(&32u16.to_le_bytes());
    image.extend_from_slice(&[0; 4]);
    image.extend_from_slice(&((size * size * 4 + mask_row * size) as u32).to_le_bytes());
    image.extend_from_slice(&[0; 16]);
    for y in (0..size).rev() {
        for x in 0..size {
            let [r, g, b, a] = rgba[y * size + x];
            image.extend_from_slice(&[byte(b), byte(g), byte(r), byte(a)]);
        }
    }
    image.resize(image.len() + mask_row * size, 0);
    image
}

/// The icon at `size` × `size`: RGBA from 0 to 1, top to bottom.
fn pixels(size: usize) -> Vec<[f32; 4]> {
    let n = size as f32;
    let (details, knurl) = (size >= DETAILS_FROM, size >= KNURL_FROM);
    // Lines are at least a pixel wide.
    let seam = (0.6 / n).max(SEAM_HALF);
    let edge = (0.9 / n).max(EDGE);
    let samples = (SAMPLES * SAMPLES) as f32;
    let mut pixels = vec![[0.0; 4]; size * size];
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0.0f32; 4];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / n;
                    let py = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / n;
                    let Some(color) = color_at(px, py, details, knurl, seam, edge) else { continue };
                    for i in 0..3 {
                        sum[i] += color[i];
                    }
                    sum[3] += 1.0;
                }
            }
            let covered = sum[3];
            if covered > 0.0 {
                pixels[y * size + x] = [sum[0] / covered, sum[1] / covered, sum[2] / covered, covered / samples];
            }
        }
    }
    pixels
}

/// The color at (`x`, `y`), or `None` outside the rounded square.
fn color_at(x: f32, y: f32, details: bool, knurl: bool, seam: f32, edge: f32) -> Option<[f32; 3]> {
    let square = rounded_box(x - 0.5, y - 0.5, SQUARE_HALF, SQUARE_HALF, SQUARE_RADIUS);
    if square > 0.0 {
        return None;
    }
    let mut color = mix(BACK_TOP, BACK_BOTTOM, y);
    let glow = (1.0 - light_distance(x, y)).max(0.0);
    color = mix(color, LIGHT, LIGHT_STRENGTH * glow * glow);
    if details && square > -edge {
        color = mix(color, GOLD, EDGE_TINT);
    }
    let below = ((x - SHADOW_CENTER.0) / SHADOW_RADII.0).hypot((y - SHADOW_CENTER.1) / SHADOW_RADII.1);
    if details && below < 1.0 {
        color = color.map(|c| c * (1.0 - SHADOW_DARKNESS * (1.0 - below)));
    }
    if !ghost(x, y) {
        return Some(color);
    }
    color = if x < 0.5 { BONE } else { BONE_SHADE };
    if eye(x, y) || eye(1.0 - x, y) {
        color = EYES;
    }
    if details && (x - 0.5).abs() <= seam && y <= SEAM_END {
        color = GOLD;
    }
    let (wx, wy) = (x - WHEEL_CENTER.0, y - WHEEL_CENTER.1);
    if details && rounded_box(wx, wy, WHEEL_HALF.0, WHEEL_HALF.1, WHEEL_RADIUS) <= 0.0 {
        let ridge = ((y - wheel_top()) / KNURL).floor() as i32 % 2 == 0;
        color = if knurl && ridge { GOLD_DARK } else { GOLD };
    }
    Some(color)
}

/// How far (`x`, `y`) is from the light's center, as a share of its reach.
fn light_distance(x: f32, y: f32) -> f32 {
    (x - LIGHT_CENTER.0).hypot((y - LIGHT_CENTER.1) * LIGHT_FLATTEN) / LIGHT_REACH
}

fn wheel_top() -> f32 {
    WHEEL_CENTER.1 - WHEEL_HALF.1
}

/// Whether (`x`, `y`) falls inside the ghost.
fn ghost(x: f32, y: f32) -> bool {
    let off = (x - 0.5).abs();
    if y < ARCH {
        return off.hypot(y - ARCH) <= ARCH_HALF;
    }
    if y <= HEM {
        let t = (y - ARCH) / (HEM - ARCH);
        return off <= ARCH_HALF + (HEM_HALF - ARCH_HALF) * t * t;
    }
    if off > HEM_HALF {
        return false;
    }
    let tooth = 2.0 * HEM_HALF / 3.0;
    let index = ((x - (0.5 - HEM_HALF)) / tooth).floor().clamp(0.0, 2.0);
    let center = 0.5 - HEM_HALF + tooth * (index + 0.5);
    let depth = if index == 1.0 { SPIKE } else { SPIKE * OUTER_SPIKE };
    y - HEM <= depth * (1.0 - (x - center).abs() / (tooth / 2.0))
}

/// Whether (`x`, `y`) falls inside the left eye: on the same side of every edge of its corners.
fn eye(x: f32, y: f32) -> bool {
    (0..EYE.len()).all(|i| {
        let (ax, ay) = EYE[i];
        let (bx, by) = EYE[(i + 1) % EYE.len()];
        (bx - ax) * (y - ay) - (by - ay) * (x - ax) >= 0.0
    })
}

/// Signed distance to a rounded box centered at the origin (negative inside).
fn rounded_box(x: f32, y: f32, half_width: f32, half_height: f32, radius: f32) -> f32 {
    let qx = x.abs() - (half_width - radius);
    let qy = y.abs() - (half_height - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The drawing as an SVG file, at its largest: every detail.
fn svg() -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{side}\" height=\"{side}\" viewBox=\"0 0 {side} {side}\">\n  \
         <title>Phantom Switch</title>\n  <!-- Drawn by tools/icon, like assets/icon.ico. -->\n{}</svg>\n",
        drawing("", "  "),
        side = SVG_SIDE,
    )
}

/// `preview` with its desktop icon drawn again, between its marks.
fn with_icon(preview: &str) -> Result<String, Box<dyn Error>> {
    const START: &str = "<!-- icon -->\n";
    const END: &str = "<!-- /icon -->";
    let start = preview.find(START).ok_or("assets/preview.svg has no <!-- icon --> mark")? + START.len();
    let end = preview[start..].find(END).ok_or("assets/preview.svg has no <!-- /icon --> mark")? + start;
    let indent = &preview[preview[..end].rfind('\n').map_or(0, |i| i + 1)..end];
    Ok(format!("{}{}{indent}{}", &preview[..start], drawing("app-", indent), &preview[end..]))
}

/// The drawing's elements at `SVG_SIDE`, each line indented by `indent`, its ids led by `id_prefix`.
fn drawing(id_prefix: &str, indent: &str) -> String {
    let at = |v: f32| number(v * SVG_SIDE);
    let hex = |c: [f32; 3]| format!("#{:02x}{:02x}{:02x}", byte(c[0]), byte(c[1]), byte(c[2]));
    let light_stops: String = [0.0f32, 0.25, 0.5, 0.75, 1.0]
        .iter()
        .map(|t| {
            let opacity = LIGHT_STRENGTH * (1.0 - t) * (1.0 - t);
            format!("\n    <stop offset=\"{t}\" stop-color=\"{}\" stop-opacity=\"{}\"/>", hex(LIGHT), number(opacity))
        })
        .collect();
    let square = (0.5 - SQUARE_HALF, 2.0 * SQUARE_HALF);
    let (edge_at, edge_side) = (square.0 + EDGE / 2.0, square.1 - EDGE);
    let tooth = 2.0 * HEM_HALF / 3.0;
    let left = 0.5 - HEM_HALF;
    let hem: String = (0..3)
        .rev()
        .flat_map(|i| {
            let depth = if i == 1 { SPIKE } else { SPIKE * OUTER_SPIKE };
            let tip = left + tooth * (i as f32 + 0.5);
            [(tip, HEM + depth), (left + tooth * i as f32, HEM)]
        })
        .map(|(x, y)| format!(" L {} {}", at(x), at(y)))
        .collect();
    let eye_path = |mirror: bool| {
        let points: Vec<String> =
            EYE.iter().map(|(x, y)| format!("{} {}", at(if mirror { 1.0 - x } else { *x }), at(*y))).collect();
        format!("M {} Z", points.join(" L "))
    };
    let wheel = (WHEEL_CENTER.0 - WHEEL_HALF.0, wheel_top(), 2.0 * WHEEL_HALF.0, 2.0 * WHEEL_HALF.1);
    let ridges: String = (0..)
        .map(|i| wheel_top() + 2.0 * KNURL * i as f32)
        .take_while(|y| *y < wheel.1 + wheel.3)
        .map(|y| {
            format!(
                "\n    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                at(wheel.0),
                at(y),
                at(wheel.2),
                at(KNURL)
            )
        })
        .collect();
    let lines = format!(
        r##"<defs>
  <linearGradient id="{id_prefix}back" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="0" y2="{side}">
    <stop offset="0" stop-color="{back_top}"/>
    <stop offset="1" stop-color="{back_bottom}"/>
  </linearGradient>
  <radialGradient id="{id_prefix}light" gradientUnits="userSpaceOnUse" cx="0" cy="0" r="{light_reach}" gradientTransform="translate({light_x} {light_y}) scale(1 {light_scale})">{light_stops}
  </radialGradient>
  <radialGradient id="{id_prefix}shadow">
    <stop offset="0" stop-color="#000" stop-opacity="{shadow}"/>
    <stop offset="1" stop-color="#000" stop-opacity="0"/>
  </radialGradient>
  <clipPath id="{id_prefix}ghost">
    <path d="M {arch_left} {arch} A {arch_half} {arch_half} 0 0 1 {arch_right} {arch} Q {arch_right} {side_mid} {hem_right} {hem}{hem_path} Q {arch_left} {side_mid} {arch_left} {arch} Z"/>
  </clipPath>
  <clipPath id="{id_prefix}wheel">
    <rect x="{wheel_x}" y="{wheel_y}" width="{wheel_w}" height="{wheel_h}" rx="{wheel_r}"/>
  </clipPath>
</defs>
<rect x="{square_at}" y="{square_at}" width="{square_side}" height="{square_side}" rx="{square_r}" fill="url(#{id_prefix}back)"/>
<rect x="{square_at}" y="{square_at}" width="{square_side}" height="{square_side}" rx="{square_r}" fill="url(#{id_prefix}light)"/>
<rect x="{edge_at}" y="{edge_at}" width="{edge_side}" height="{edge_side}" rx="{edge_r}" fill="none" stroke="{gold}" stroke-opacity="{edge_tint}" stroke-width="{edge}"/>
<ellipse cx="{shadow_x}" cy="{shadow_y}" rx="{shadow_rx}" ry="{shadow_ry}" fill="url(#{id_prefix}shadow)"/>
<g clip-path="url(#{id_prefix}ghost)">
  <rect x="0" y="0" width="{half}" height="{side}" fill="{bone}"/>
  <rect x="{half}" y="0" width="{half}" height="{side}" fill="{bone_shade}"/>
  <rect x="{seam_x}" y="0" width="{seam_w}" height="{seam_end}" fill="{gold}"/>
</g>
<g clip-path="url(#{id_prefix}wheel)">
  <rect x="{wheel_x}" y="{wheel_y}" width="{wheel_w}" height="{wheel_h}" fill="{gold}"/>
  <g fill="{gold_dark}">{ridges}
  </g>
</g>
<g fill="{eyes}">
  <path d="{left_eye}"/>
  <path d="{right_eye}"/>
</g>
"##,
        side = number(SVG_SIDE),
        half = at(0.5),
        back_top = hex(BACK_TOP),
        back_bottom = hex(BACK_BOTTOM),
        light_reach = at(LIGHT_REACH),
        light_x = at(LIGHT_CENTER.0),
        light_y = at(LIGHT_CENTER.1),
        light_scale = number(1.0 / LIGHT_FLATTEN),
        shadow = number(SHADOW_DARKNESS),
        arch_left = at(0.5 - ARCH_HALF),
        arch_right = at(0.5 + ARCH_HALF),
        arch = at(ARCH),
        arch_half = at(ARCH_HALF),
        side_mid = at((ARCH + HEM) / 2.0),
        hem_right = at(0.5 + HEM_HALF),
        hem = at(HEM),
        hem_path = hem,
        wheel_x = at(wheel.0),
        wheel_y = at(wheel.1),
        wheel_w = at(wheel.2),
        wheel_h = at(wheel.3),
        wheel_r = at(WHEEL_RADIUS),
        square_at = at(square.0),
        square_side = at(square.1),
        square_r = at(SQUARE_RADIUS),
        edge_at = at(edge_at),
        edge_side = at(edge_side),
        edge_r = at(SQUARE_RADIUS - EDGE / 2.0),
        edge = at(EDGE),
        edge_tint = number(EDGE_TINT),
        gold = hex(GOLD),
        gold_dark = hex(GOLD_DARK),
        shadow_x = at(SHADOW_CENTER.0),
        shadow_y = at(SHADOW_CENTER.1),
        shadow_rx = at(SHADOW_RADII.0),
        shadow_ry = at(SHADOW_RADII.1),
        bone = hex(BONE),
        bone_shade = hex(BONE_SHADE),
        seam_x = at(0.5 - SEAM_HALF),
        seam_w = at(2.0 * SEAM_HALF),
        seam_end = at(SEAM_END),
        eyes = hex(EYES),
        left_eye = eye_path(false),
        right_eye = eye_path(true),
    );
    lines.lines().map(|line| format!("{indent}{line}\n")).collect()
}

/// `value` with at most three decimals, without trailing zeros.
fn number(value: f32) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}
