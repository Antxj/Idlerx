//! Development builds only (no signed extension embedded): installs
//! Idlerx's extension from its source folder for this Firefox session,
//! through Firefox's debugger protocol (packets "length:json").

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

fn read(s: &mut TcpStream, buf: &mut Vec<u8>) -> Option<serde_json::Value> {
    loop {
        if let Some(colon) = buf.iter().position(|&b| b == b':') {
            let n: usize = std::str::from_utf8(&buf[..colon]).ok()?.parse().ok()?;
            if buf.len() > colon + n {
                let v = serde_json::from_slice(&buf[colon + 1..=colon + n]).ok();
                buf.drain(..=colon + n);
                return v;
            }
        }
        let mut chunk = [0u8; 16 * 1024];
        let got = s.read(&mut chunk).ok()?;
        if got == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..got]);
    }
}

fn write(s: &mut TcpStream, v: serde_json::Value) -> Option<()> {
    let b = v.to_string();
    s.write_all(format!("{}:{}", b.len(), b).as_bytes()).ok()
}

/// Waits for the debugger of the Firefox just started and installs the
/// extension. Err = why not.
pub fn install_extension(port: u16) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(30);
    let mut s = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if Instant::now() < until => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(20)));
    let mut buf = Vec::new();
    read(&mut s, &mut buf).ok_or("sem resposta do depurador")?;
    write(
        &mut s,
        serde_json::json!({ "to": "root", "type": "getRoot" }),
    )
    .ok_or("depurador")?;
    let addons = loop {
        let v = read(&mut s, &mut buf).ok_or("sem resposta do depurador")?;
        if let Some(a) = v["addonsActor"].as_str() {
            break a.to_string();
        }
    };
    let path = std::fs::canonicalize(
        super::profile::extension_source()
            .ok_or("extensão do Idlerx não assinada e sem a pasta do código")?,
    )
    .map_err(|e| e.to_string())?
    .to_string_lossy()
    .trim_start_matches(r"\\?\")
    .to_string();
    write(
        &mut s,
        serde_json::json!({ "to": addons, "type": "installTemporaryAddon", "addonPath": path }),
    )
    .ok_or("depurador")?;
    loop {
        let v = read(&mut s, &mut buf).ok_or("sem resposta do depurador")?;
        if v.get("addon").is_some() {
            return Ok(());
        }
        if let Some(e) = v.get("error") {
            return Err(format!("{e} {}", v["message"]));
        }
    }
}
