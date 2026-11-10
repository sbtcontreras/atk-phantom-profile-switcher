//! The `.exe`'s resources: its icon, its version information and its manifest (per-monitor
//! scaling, modern controls). The icon is drawn right here, so the repository carries no images:
//! a rounded square in an indigo gradient with a white toggle switch.

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

/// Phantom Switch's icon: a ghost shaped like a mouse (the seam between its buttons and its wheel
/// on top, a ghost's wavy hem below) on a rounded square in an indigo gradient. RGBA from 0 to 1,
/// top to bottom, with 4 × 4 samples per pixel.
fn brand(size: usize) -> Vec<[f32; 4]> {
    let n = size as f32;
    let top_left = [0.39, 0.40, 0.95];
    let bottom_right = [0.42, 0.16, 0.85];
    // Below 32 px the seam and the wheel would be a smudge: the ghost keeps its eyes.
    let details = size >= 32;
    // The seam is at least a pixel wide.
    let seam = (0.6 / n).max(0.009);
    let mut pixels = vec![[0.0; 4]; size * size];
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0.0f32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = (x as f32 + (sx as f32 + 0.5) / 4.0) / n;
                    let py = (y as f32 + (sy as f32 + 0.5) / 4.0) / n;
                    if rounded_box(px - 0.5, py - 0.5, 0.47, 0.47, 0.22) > 0.0 {
                        continue;
                    }
                    let t = (px + py) / 2.0;
                    let mut color: [f32; 3] =
                        std::array::from_fn(|i| top_left[i] + (bottom_right[i] - top_left[i]) * t);
                    // A soft shadow under the ghost, as if it floated.
                    let below = ((px - 0.5) / 0.2).hypot((py - 0.845) / 0.03);
                    if details && below < 1.0 {
                        color = color.map(|c| c * (1.0 - 0.3 * (1.0 - below)));
                    }
                    if ghost(px, py) {
                        color = [1.0, 1.0, 1.0];
                        let eye = |cx: f32| ((px - cx) / 0.04).hypot((py - 0.47) / 0.055) <= 1.0;
                        if eye(0.415) || eye(0.585) {
                            color = [0.12, 0.11, 0.29];
                        }
                        if details && (px - 0.5).abs() <= seam && py <= 0.37 {
                            color = [0.62, 0.65, 0.97];
                        }
                        if details && rounded_box(px - 0.5, py - 0.29, 0.024, 0.048, 0.024) <= 0.0 {
                            color = [0.45, 0.47, 0.92];
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

/// Whether (`x`, `y`), from 0 to 1, falls inside the ghost: a dome on top (the mouse's front),
/// straight sides, and three scallops hanging from its hem.
fn ghost(x: f32, y: f32) -> bool {
    const LEFT: f32 = 0.27;
    const RIGHT: f32 = 0.73;
    const DOME: f32 = 0.42;
    const HEM: f32 = 0.7;
    let half = (RIGHT - LEFT) / 2.0;
    if y < DOME {
        return (x - 0.5).hypot(y - DOME) <= half;
    }
    if !(LEFT..=RIGHT).contains(&x) {
        return false;
    }
    if y <= HEM {
        return true;
    }
    let bump = (RIGHT - LEFT) / 6.0;
    let scallop = ((x - LEFT) / (2.0 * bump)).floor().clamp(0.0, 2.0);
    (x - (LEFT + bump * (2.0 * scallop + 1.0))).hypot(y - HEM) <= bump
}

/// Signed distance to a rounded box centered at the origin (negative inside).
fn rounded_box(x: f32, y: f32, half_width: f32, half_height: f32, radius: f32) -> f32 {
    let qx = x.abs() - (half_width - radius);
    let qy = y.abs() - (half_height - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}
