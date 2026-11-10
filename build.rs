//! Puts the `.exe`'s resources in it: the icon (`assets/icon.ico`, drawn by `tools/icon`), the
//! manifest (per-monitor scaling, modern controls) and the version information from `Cargo.toml`.

use std::error::Error;
use std::path::Path;
use std::{env, fs};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=assets");
    if env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    let rc = Path::new(&env::var("OUT_DIR")?).join("app.rc");
    fs::write(&rc, resources()?)?;
    embed_resource::compile(&rc, embed_resource::NONE).manifest_required()?;
    Ok(())
}

/// The resource script, written here for the version Cargo builds.
fn resources() -> Result<String, Box<dyn Error>> {
    let assets = Path::new(&env::var("CARGO_MANIFEST_DIR")?).join("assets");
    // In an .rc string, a backslash escapes: Windows paths need it doubled.
    let path = |file: &str| assets.join(file).display().to_string().replace('\\', "\\\\");
    let version = env::var("CARGO_PKG_VERSION")?;
    let (major, minor, patch) = (
        env::var("CARGO_PKG_VERSION_MAJOR")?,
        env::var("CARGO_PKG_VERSION_MINOR")?,
        env::var("CARGO_PKG_VERSION_PATCH")?,
    );
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
        icon = path("icon.ico"),
        manifest = path("app.manifest"),
    ))
}
