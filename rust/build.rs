// Embeds the icon, the manifest and the version information into the .exe.
// The .rc file is generated here so its version always matches Cargo.toml.
//
// Also embeds Idlerx's Firefox extension signed by Mozilla
// (assets/idlerx-firefox.xpi, made by the release workflow; without it a
// development build installs the extension from firefox/extension for each
// session) and the official OpenVPN installer (assets/openvpn.msi), used by
// the VPN of groups. The MSI is NOT in the repository: build-release.ps1
// downloads it and verifies its SHA-256 and signature. Without the file the
// executable has no installer (the app points to the download page).

use std::path::{Path, PathBuf};

/// OpenVPN installer version expected in assets/openvpn.msi (kept in sync
/// with build-release.ps1, which downloads it and verifies the hash).
const OPENVPN_MSI_VERSION: &str = "2.7.6-I001";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=assets/openvpn.msi");
    println!("cargo:rerun-if-changed=assets/idlerx-firefox.xpi");
    embed_openvpn_msi();
    embed_optional(
        "assets/idlerx-firefox.xpi",
        "idlerx-firefox.xpi",
        "IDLERX_FIREFOX_XPI",
    );

    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }
    // with the GNU toolchain the C runtime is not static: the single
    // executable needs MSVC
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        panic!(
            "Idlerx must be built with the MSVC toolchain: run \
             `rustup override set stable-x86_64-pc-windows-msvc` in this folder \
             (requires Visual Studio Build Tools)"
        );
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

/// include_bytes! needs a file that always exists: without the MSI an empty
/// one is used (the app sees it at run time).
fn embed_openvpn_msi() {
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("openvpn.msi");
    let source = Path::new("assets/openvpn.msi");
    if source.exists() {
        std::fs::copy(source, &out).expect("copy the OpenVPN installer");
    } else {
        std::fs::write(&out, b"").expect("write an empty installer");
    }
    println!("cargo:rustc-env=IDLERX_OPENVPN_MSI={}", out.display());
    println!("cargo:rustc-env=IDLERX_OPENVPN_MSI_VERSION={OPENVPN_MSI_VERSION}");
}

/// Embeds an optional asset (an empty file when it is missing).
fn embed_optional(source: &str, name: &str, env: &str) {
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join(name);
    if Path::new(source).exists() {
        std::fs::copy(source, &out).expect("copy asset");
    } else {
        std::fs::write(&out, b"").expect("write an empty asset");
    }
    println!("cargo:rustc-env={env}={}", out.display());
}
