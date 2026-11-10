//! The `.exe`'s resources: its icon, its version information and its manifest (per-monitor
//! scaling, modern controls). The icon is drawn right here from shapes rather than kept as a
//! bitmap; `docs/icon.svg` is the same drawing, for the README.

use std::error::Error;
use std::path::Path;
use std::{env, fs};

/// The sides of the `.exe`'s icon: from file lists to Explorer's large icons.
const SIZES: [usize; 8] = [16, 20, 24, 32, 40, 48, 64, 128];

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="sbtcontreras.PhantomSwitch" version="1.0.0.0"/>
  <description>Phantom Switch</description>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    let out = env::var("OUT_DIR")?;
    let out = Path::new(&out);
    fs::write(out.join("app.ico"), ico())?;
    fs::write(out.join("app.manifest"), MANIFEST)?;
    let rc = out.join("app.rc");
    fs::write(&rc, resources(out)?)?;
    embed_resource::compile(&rc, embed_resource::NONE).manifest_required()?;
    Ok(())
}

fn resources(out: &Path) -> Result<String, Box<dyn Error>> {
    // In an .rc string, a backslash escapes: Windows paths need it doubled.
    let path = |file: &str| out.join(file).display().to_string().replace('\\', "\\\\");
    let version = env::var("CARGO_PKG_VERSION")?;
    let numbers: Vec<&str> = version.split(['.', '-']).take(3).collect();
    let [major, minor, patch] = numbers[..] else { return Err("the version is not X.Y.Z".into()) };
    let repository = env::var("CARGO_PKG_REPOSITORY")?;
    Ok(format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "sbtcontreras"
      VALUE "FileDescription", "Phantom Switch"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "PhantomSwitch"
      VALUE "LegalCopyright", "MIT License - {repository}"
      VALUE "OriginalFilename", "PhantomSwitch.exe"
      VALUE "ProductName", "Phantom Switch"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#,
        icon = path("app.ico"),
        manifest = path("app.manifest"),
    ))
}

/// An .ico with a 32-bit bitmap per size.
fn ico() -> Vec<u8> {
    let images: Vec<Vec<u8>> = SIZES.iter().map(|size| bitmap(*size, &brand(*size))).collect();
    let mut file = Vec::new();
    file.extend_from_slice(&[0, 0, 1, 0]);
    file.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len();
    for (size, image) in SIZES.iter().zip(&images) {
        let side = if *size >= 256 { 0 } else { *size as u8 };
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
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    for y in (0..size).rev() {
        for x in 0..size {
            let [r, g, b, a] = rgba[y * size + x];
            image.extend_from_slice(&[byte(b), byte(g), byte(r), byte(a)]);
        }
    }
    image.resize(image.len() + mask_row * size, 0);
    image
}

/// The look of the ATK Phantom itself: a bone-white shell with gold details, in warm dark light.
const BONE: [f32; 3] = [0.95, 0.92, 0.86];
/// The right half of the shell, a shade darker, as a second panel catching less light.
const BONE_SHADE: [f32; 3] = [0.86, 0.82, 0.74];
const GOLD: [f32; 3] = [0.8, 0.64, 0.36];
const GOLD_DARK: [f32; 3] = [0.55, 0.42, 0.22];
const EYES: [f32; 3] = [0.09, 0.075, 0.06];
const BACK_TOP: [f32; 3] = [0.2, 0.18, 0.15];
const BACK_BOTTOM: [f32; 3] = [0.075, 0.065, 0.055];
const LIGHT: [f32; 3] = [0.45, 0.39, 0.29];

/// Phantom Switch's icon: a fierce ghost shaped like a mouse, in the ATK Phantom's own look (a
/// bone-white shell in two panels with the mouse's rounded front, a gold seam and wheel, angular
/// eyes, a hem of spikes) on a rounded square of warm dark with a light from above and a thin
/// gold edge. RGBA from 0 to 1, top to bottom, with 4 × 4 samples per pixel.
fn brand(size: usize) -> Vec<[f32; 4]> {
    let n = size as f32;
    // Below 32 px the edge, the seam and the wheel would be a smudge, and the wheel's knurl
    // below 64: the ghost keeps its eyes.
    let details = size >= 32;
    let knurl = size >= 64;
    // Lines are at least a pixel wide.
    let line = (0.6 / n).max(0.009);
    let edge = (0.9 / n).max(0.012);
    let mut pixels = vec![[0.0; 4]; size * size];
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0.0f32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = (x as f32 + (sx as f32 + 0.5) / 4.0) / n;
                    let py = (y as f32 + (sy as f32 + 0.5) / 4.0) / n;
                    let square = rounded_box(px - 0.5, py - 0.5, 0.47, 0.47, 0.22);
                    if square > 0.0 {
                        continue;
                    }
                    let mut color = mix(BACK_TOP, BACK_BOTTOM, py);
                    // The light from above, as in the room the mouse sits in.
                    let glow = (1.0 - (px - 0.5).hypot((py + 0.05) * 1.2) / 0.8).max(0.0);
                    color = mix(color, LIGHT, 0.45 * glow * glow);
                    if details && square > -edge {
                        color = mix(color, GOLD, 0.6);
                    }
                    // A soft shadow under the ghost, as if it floated.
                    let below = ((px - 0.5) / 0.22).hypot((py - 0.875) / 0.03);
                    if details && below < 1.0 {
                        color = color.map(|c| c * (1.0 - 0.45 * (1.0 - below)));
                    }
                    if ghost(px, py) {
                        color = if px < 0.5 { BONE } else { BONE_SHADE };
                        if blade(px, py) || blade(1.0 - px, py) {
                            color = EYES;
                        }
                        if details && (px - 0.5).abs() <= line && py <= 0.4 {
                            color = GOLD;
                        }
                        if details && rounded_box(px - 0.5, py - 0.3, 0.024, 0.05, 0.012) <= 0.0 {
                            let ridge = ((py - 0.25) / 0.013).floor() as i32 % 2 == 0;
                            color = if knurl && ridge { GOLD_DARK } else { GOLD };
                        }
                    }
                    for i in 0..3 {
                        sum[i] += color[i];
                    }
                    sum[3] += 1.0;
                }
            }
            let covered = sum[3];
            pixels[y * size + x] = if covered > 0.0 {
                [sum[0] / covered, sum[1] / covered, sum[2] / covered, covered / 16.0]
            } else {
                [0.0; 4]
            };
        }
    }
    pixels
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Whether (`x`, `y`), from 0 to 1, falls inside the ghost: the mouse's rounded front on top,
/// sides that widen like a cape, and three spikes at its hem, the middle one longest.
fn ghost(x: f32, y: f32) -> bool {
    /// Where the front meets the sides, and its half width there.
    const ARCH: f32 = 0.45;
    const ARCH_HALF: f32 = 0.22;
    /// The radius of the front's two arcs: equal to the half width, a round front.
    const RADIUS: f32 = 0.22;
    const HEM: f32 = 0.69;
    const HEM_HALF: f32 = 0.27;
    const SPIKE: f32 = 0.16;
    let off = (x - 0.5).abs();
    if y < ARCH {
        // Inside both arcs, each centered on the other side.
        let center = ARCH_HALF - RADIUS;
        return (off + center).hypot(y - ARCH) <= RADIUS && (off - center).hypot(y - ARCH) <= RADIUS;
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
    let depth = if index == 1.0 { SPIKE } else { SPIKE * 0.8 };
    y - HEM <= depth * (1.0 - (x - center).abs() / (tooth / 2.0))
}

/// Whether (`x`, `y`) falls inside the left eye: an angular blade slanting down toward the middle,
/// like the cuts in the mouse's shell. The right eye is its mirror.
fn blade(x: f32, y: f32) -> bool {
    const CORNERS: [(f32, f32); 4] = [(0.355, 0.445), (0.47, 0.495), (0.462, 0.528), (0.372, 0.5)];
    // Inside a convex shape: on the same side of every edge, taken clockwise.
    (0..4).all(|i| {
        let (ax, ay) = CORNERS[i];
        let (bx, by) = CORNERS[(i + 1) % 4];
        (bx - ax) * (y - ay) - (by - ay) * (x - ax) >= 0.0
    })
}

/// Signed distance to a rounded box centered at the origin (negative inside).
fn rounded_box(x: f32, y: f32, half_width: f32, half_height: f32, radius: f32) -> f32 {
    let qx = x.abs() - (half_width - radius);
    let qy = y.abs() - (half_height - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}
