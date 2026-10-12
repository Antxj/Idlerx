//! OpenVPN Community: where it is installed, and its installation from the
//! official MSI embedded in the executable.
//!
//! The MSI is the package signed by OpenVPN Inc., redistributed without
//! modification (see THIRD-PARTY-LICENSES.txt). The installation is silent
//! and picks a minimal set of components: core, service and the network
//! drivers - without the OpenVPN GUI, which would add a tray icon of its own.

use std::path::PathBuf;

/// Embedded MSI (empty when the build did not find assets/openvpn.msi).
static MSI: &[u8] = include_bytes!(env!("IDLERX_OPENVPN_MSI"));
pub const MSI_VERSION: &str = env!("IDLERX_OPENVPN_MSI_VERSION");

/// Installed components (names checked against the MSI Feature table).
const FEATURES: &str =
    "ADDLOCAL=OpenVPN,OpenVPN.Service,Drivers,Drivers.TAPWindows6,Drivers.OvpnDco";
/// msiexec: success, but Windows asks for a restart.
const ERROR_SUCCESS_REBOOT_REQUIRED: u32 = 3010;
const ERROR_INSTALL_USEREXIT: u32 = 1602;

/// Installation folder of OpenVPN (registry, written by the official
/// installer), when openvpn.exe is there.
pub fn openvpn_dir() -> Option<PathBuf> {
    use super::elevate::wide;
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
    };
    let key = wide(r"SOFTWARE\OpenVPN");
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            std::ptr::null(), // default value: the installation folder
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut _,
            &mut len,
        )
    };
    let from_registry = (ok == 0).then(|| {
        let n = (len as usize / 2).saturating_sub(1).min(buf.len());
        PathBuf::from(String::from_utf16_lossy(&buf[..n]))
    });
    from_registry
        .into_iter()
        .chain([PathBuf::from(r"C:\Program Files\OpenVPN")])
        .find(|d| d.join("bin").join("openvpn.exe").exists())
}

/// This build carries the installer.
pub fn embedded() -> bool {
    !MSI.is_empty()
}

fn log_path() -> PathBuf {
    super::dir().join("openvpn-install.log")
}

/// Installs OpenVPN (blocking: run it in a thread). Windows asks for
/// administrator permission (UAC). Ok(true) = a restart is recommended.
pub fn install() -> Result<bool, String> {
    if !embedded() {
        return Err(tr!(
            "Esta compilação não tem o instalador do OpenVPN embutido.",
            "This build does not include the OpenVPN installer."
        )
        .into());
    }
    let msi = std::env::temp_dir().join(format!("idlerx-openvpn-{MSI_VERSION}.msi"));
    std::fs::write(&msi, MSI).map_err(|e| {
        trf!(
            "Não consegui gravar o instalador em disco: {e}",
            "Could not write the installer to disk: {e}"
        )
    })?;
    let log = log_path();
    let _ = std::fs::create_dir_all(super::dir());
    let msiexec =
        PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
            .join("System32")
            .join("msiexec.exe");
    let params = format!(
        "/i {} /qn /norestart {FEATURES} /l*v {}",
        super::service::arg(&msi.to_string_lossy()),
        super::service::arg(&log.to_string_lossy())
    );
    let result = super::elevate::run_as_admin(&msiexec, &params);
    let _ = std::fs::remove_file(&msi);
    match result? {
        0 => Ok(false),
        ERROR_SUCCESS_REBOOT_REQUIRED => Ok(true),
        ERROR_INSTALL_USEREXIT => Err(tr!(
            "A instalação foi cancelada.",
            "The installation was cancelled."
        )
        .into()),
        code => Err(trf!(
            "A instalação falhou (código {code}). Detalhes em:\n{}",
            "The installation failed (code {code}). Details in:\n{}",
            log.display()
        )),
    }
}
