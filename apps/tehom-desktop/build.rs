//! Embeds the Windows icon and version information into tehom-desktop.exe.
//!
//! The application manifest (asInvoker, per-monitor-v2 DPI awareness, Common
//! Controls v6, Windows 10/11) is not written here: gpui links its own
//! manifest resource into every binary, and a second one is a duplicate
//! resource error at link time.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = dir.join("../../assets/brand/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let version = |part| std::env::var(format!("CARGO_PKG_VERSION_{part}")).unwrap();
    let numeric = format!(
        "{},{},{},0",
        version("MAJOR"),
        version("MINOR"),
        version("PATCH")
    );
    let text = std::env::var("CARGO_PKG_VERSION").unwrap();
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Tehom contributors"
      VALUE "FileDescription", "Tehom"
      VALUE "FileVersion", "{text}"
      VALUE "InternalName", "tehom-desktop"
      VALUE "LegalCopyright", "Copyright (C) Tehom contributors. AGPL-3.0-or-later"
      VALUE "OriginalFilename", "tehom-desktop.exe"
      VALUE "ProductName", "Tehom"
      VALUE "ProductVersion", "{text}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "\\\\"),
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("tehom-desktop.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
