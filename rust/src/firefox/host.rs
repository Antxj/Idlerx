//! Native messaging: Firefox starts `Idlerx.exe` (registered as the
//! extension's native app) and talks to it through stdin/stdout. That
//! process only relays the messages to the Idlerx that started this Firefox,
//! through a local socket: its port, a one-time key and the network name
//! come in the environment, inherited from Firefox.

use std::io::{Read, Write};
use std::net::TcpStream;

pub const HOST_NAME: &str = "io.github.antxj.idlerx";
pub const ENV_PORT: &str = "IDLERX_FF_PORT";
pub const ENV_TOKEN: &str = "IDLERX_FF_TOKEN";
pub const ENV_NET: &str = "IDLERX_FF_NET";

/// Started by Firefox as the native app (its arguments: the manifest and the
/// extension's id).
pub fn is_host_call(args: &[String]) -> bool {
    args.iter().any(|a| a == super::profile::EXTENSION_ID)
}

/// Writes a message in the native messaging format: length, then JSON.
pub fn frame(json: &[u8]) -> Vec<u8> {
    let mut out = (json.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(json);
    out
}

/// Reads one message (None at the end of the stream).
pub fn read_frame(r: &mut impl Read) -> Option<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).ok()?;
    let n = u32::from_le_bytes(len) as usize;
    if n > 64 << 20 {
        return None;
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// Development: why the relay ended, in a file next to the profiles.
fn trace(what: &str) {
    if let Ok(path) = std::env::var("IDLERX_RELAY_TRACE") {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(
                f,
                "{:?} pid={} {what}",
                std::time::SystemTime::now(),
                std::process::id()
            );
        }
    }
}

/// The relay: runs until Firefox or Idlerx goes away.
pub fn run() -> ! {
    trace("start");
    let port: u16 = std::env::var(ENV_PORT)
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    let token = std::env::var(ENV_TOKEN).unwrap_or_default();
    let net = std::env::var(ENV_NET).unwrap_or_default();
    let Ok(mut sock) = TcpStream::connect(("127.0.0.1", port)) else {
        std::process::exit(1);
    };
    let _ = sock.set_nodelay(true);
    let hello = serde_json::json!({ "token": token, "net": net }).to_string();
    if sock.write_all(&frame(hello.as_bytes())).is_err() {
        std::process::exit(1);
    }
    let Ok(mut to_firefox) = sock.try_clone() else {
        std::process::exit(1);
    };
    // Idlerx -> Firefox
    std::thread::spawn(move || {
        let mut out = std::io::stdout().lock();
        let mut buf = [0u8; 64 * 1024];
        loop {
            match to_firefox.read(&mut buf) {
                Ok(0) => {
                    trace("idlerx closed the socket");
                    std::process::exit(0)
                }
                Err(e) => {
                    trace(&format!("socket read error {e}"));
                    std::process::exit(0)
                }
                Ok(n) => {
                    if let Err(e) = out.write_all(&buf[..n]).and_then(|_| out.flush()) {
                        trace(&format!("stdout write error {e}"));
                        std::process::exit(0);
                    }
                }
            }
        }
    });
    // Firefox -> Idlerx
    let mut input = std::io::stdin().lock();
    let mut buf = [0u8; 64 * 1024];
    loop {
        match input.read(&mut buf) {
            Ok(0) => {
                trace("firefox closed stdin");
                std::process::exit(0)
            }
            Err(e) => {
                trace(&format!("stdin read error {e}"));
                std::process::exit(0)
            }
            Ok(n) => {
                if let Err(e) = sock.write_all(&buf[..n]) {
                    trace(&format!("socket write error {e}"));
                    std::process::exit(0);
                }
            }
        }
    }
}

/// Registers this executable as the extension's native app (current user):
/// a manifest next to the profiles, and its path in the registry.
pub fn register() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let manifest = super::profile::root().join("native-host.json");
    std::fs::create_dir_all(super::profile::root()).map_err(|e| e.to_string())?;
    let json = serde_json::json!({
        "name": HOST_NAME,
        "description": "Idlerx",
        "path": exe,
        "type": "stdio",
        "allowed_extensions": [super::profile::EXTENSION_ID],
    });
    let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    if std::fs::read_to_string(&manifest).ok().as_deref() != Some(text.as_str()) {
        std::fs::write(&manifest, text).map_err(|e| e.to_string())?;
    }
    set_registry(
        &format!(r"Software\Mozilla\NativeMessagingHosts\{HOST_NAME}"),
        &manifest.to_string_lossy(),
    )
}

fn set_registry(key: &str, value: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    let k = crate::single::wide(key);
    let v = crate::single::wide(value);
    let mut h: HKEY = std::ptr::null_mut();
    unsafe {
        let r = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            k.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut h,
            std::ptr::null_mut(),
        );
        if r != 0 {
            return Err(format!("registro ({r})"));
        }
        let r = RegSetValueExW(
            h,
            std::ptr::null(),
            0,
            REG_SZ,
            v.as_ptr() as *const u8,
            (v.len() * 2) as u32,
        );
        RegCloseKey(h);
        if r != 0 {
            return Err(format!("registro ({r})"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let f = frame(br#"{"a":1}"#);
        assert_eq!(&f[..4], &7u32.to_le_bytes());
        let mut r = std::io::Cursor::new([f.clone(), f].concat());
        assert_eq!(read_frame(&mut r).unwrap(), br#"{"a":1}"#);
        assert_eq!(read_frame(&mut r).unwrap(), br#"{"a":1}"#);
        assert!(read_frame(&mut r).is_none());
    }
}
