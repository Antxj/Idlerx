// Embeds the icon, the manifest and the version information into the .exe.
// The .rc file is generated here so its version always matches Cargo.toml.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");

    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let mut n: Vec<u32> = version
        .split(['-', '+'])
        .next()
        .unwrap_or("0")
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    n.resize(4, 0);
    let commas = format!("{},{},{},{}", n[0], n[1], n[2], n[3]);

    let assets =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir")).join("assets");
    let path = |f: &str| assets.join(f).display().to_string().replace('\\', "/");
    let rc = format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {commas}
PRODUCTVERSION {commas}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Idlerx"
      VALUE "FileDescription", "Idlerx - isolated browser sessions side by side"
      VALUE "FileVersion", "{version}"
      VALUE "LegalCopyright", "GPL-3.0-or-later"
      VALUE "ProductName", "Idlerx"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "Idlerx.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 0x04B0
  END
END
"#,
        icon = path("icon.ico"),
        manifest = path("app.manifest"),
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("app.rc");
    std::fs::write(&out, rc).expect("write app.rc");
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_required()
        .expect("embed resources");
}
