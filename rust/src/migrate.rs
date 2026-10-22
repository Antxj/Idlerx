//! Moving each account from the previous engine (WebView2, up to 0.10) to
//! Firefox: its logins (cookies, so the games stay logged in) and its saved
//! passwords. WebView2 kept them in Chromium's format: SQLite files whose
//! values are encrypted with AES-256-GCM, with a key protected by Windows
//! (DPAPI, current user) in "Local State".
//!
//! Each account's data was in one of three places, depending on how it
//! last ran: the shared browser (economy mode), its group's VPN browser, or
//! its own folder. The newest one is used. The old folders stay until the
//! user deletes them in Settings (a game may keep data elsewhere, such as
//! IndexedDB, that does not come over).

use crate::passwords::Login;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// What an account brings from WebView2.
#[derive(Default)]
pub struct Moved {
    /// Cookies in the shape Firefox's extension API takes (`cookies.set`).
    pub cookies: Vec<Value>,
    pub logins: Vec<Login>,
    /// localStorage of each site (many browser games keep their saves
    /// there): origin -> (key, value).
    pub storage: std::collections::HashMap<String, Vec<(String, String)>>,
}

/// Folders of the previous engine (everything in the profiles folder that
/// is not Firefox's).
pub fn old_dirs() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(crate::config::profiles_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_name() != "firefox" && e.path().is_dir())
        .map(|e| e.path())
        .collect()
}

/// Where an account's WebView2 data may be: (profile folder, Local State).
fn candidates(profile: &str) -> Vec<(PathBuf, PathBuf)> {
    let root = crate::config::profiles_dir();
    let mut out = Vec::new();
    let own = root.join(profile).join("EBWebView");
    out.push((own.join("Default"), own.join("Local State")));
    for dir in old_dirs() {
        let name = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
        if name == "_shared" || name.starts_with("_vpn_") {
            let ebw = dir.join("EBWebView");
            out.push((ebw.join(format!("WV2Profile_{profile}")), ebw.join("Local State")));
        }
    }
    out
}

fn modified(p: &Path) -> std::time::SystemTime {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::UNIX_EPOCH)
}

/// The account's logins and passwords in WebView2 (None if it had none).
pub fn read(profile: &str) -> Option<Moved> {
    // the place used last
    let (dir, state) = candidates(profile)
        .into_iter()
        .filter(|(d, s)| d.join("Network").join("Cookies").exists() && s.exists())
        .max_by_key(|(d, _)| modified(&d.join("Network").join("Cookies")))?;
    let key = key(&state)?;
    let cookies = read_cookies(&dir.join("Network").join("Cookies"), &key).unwrap_or_default();
    let logins = read_logins(&dir.join("Login Data"), &key).unwrap_or_default();
    let storage = read_storage(&dir.join("Local Storage").join("leveldb"));
    Some(Moved {
        cookies,
        logins,
        storage,
    })
}

/// The AES key of a WebView2 browser (from its Local State).
fn key(local_state: &Path) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let json: Value = serde_json::from_slice(&std::fs::read(local_state).ok()?).ok()?;
    let b64 = json["os_crypt"]["encrypted_key"].as_str()?;
    let raw = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let protected = raw.strip_prefix(b"DPAPI")?;
    let key = crate::vpn::secrets::dpapi(protected, false)?;
    (key.len() == 32).then_some(key)
}

/// Decrypts a Chromium value ("v10"/"v11", nonce, ciphertext and tag).
fn decrypt(key: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    if !(data.starts_with(b"v10") || data.starts_with(b"v11")) || data.len() < 3 + 12 + 16 {
        return None;
    }
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(key).ok()?;
    let nonce = aes_gcm::Nonce::from_slice(&data[3..15]);
    cipher.decrypt(nonce, &data[15..]).ok()
}

/// Opens a copy of a SQLite file (the original may still be in use).
fn open_copy(path: &Path) -> Option<(rusqlite::Connection, PathBuf)> {
    let copy = std::env::temp_dir().join(format!("idlerx-move-{}.db", crate::config::new_id()));
    std::fs::copy(path, &copy).ok()?;
    let db = rusqlite::Connection::open_with_flags(&copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    Some((db, copy))
}

/// Seconds since 1970 of a Chromium time (microseconds since 1601).
fn unix_time(chromium: i64) -> i64 {
    chromium / 1_000_000 - 11_644_473_600
}

fn read_cookies(path: &Path, key: &[u8]) -> Option<Vec<Value>> {
    let (db, copy) = open_copy(path)?;
    // from this version on, a value starts with the SHA-256 of its host
    let version: i64 = db
        .query_row("SELECT value FROM meta WHERE key = 'version'", [], |r| {
            r.get::<_, String>(0)
        })
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut out = Vec::new();
    {
        let mut st = db
            .prepare(
                "SELECT host_key, name, value, encrypted_value, path, expires_utc, is_secure, \
                 is_httponly, samesite, has_expires, top_frame_site_key FROM cookies",
            )
            .ok()?;
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, bool>(6)?,
                    r.get::<_, bool>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, bool>(9)?,
                    r.get::<_, String>(10).unwrap_or_default(),
                ))
            })
            .ok()?;
        for row in rows.flatten() {
            let (host, name, plain, enc, path, expires, secure, http_only, same_site, has_expires, partition) = row;
            // partitioned (third-party) cookies stay behind
            if !partition.is_empty() {
                continue;
            }
            let value = if enc.is_empty() {
                plain
            } else {
                let Some(mut v) = decrypt(key, &enc) else { continue };
                if version >= 24 && v.len() >= 32 {
                    use sha2::Digest;
                    if v[..32] == sha2::Sha256::digest(host.as_bytes())[..] {
                        v.drain(..32);
                    }
                }
                match String::from_utf8(v) {
                    Ok(s) => s,
                    Err(_) => continue,
                }
            };
            let expiry = has_expires.then(|| unix_time(expires));
            if expiry.is_some_and(|e| e <= now) {
                continue;
            }
            let bare = host.trim_start_matches('.');
            let scheme = if secure { "https" } else { "http" };
            let mut c = json!({
                "url": format!("{scheme}://{bare}{path}"),
                "name": name,
                "value": value,
                "path": path,
                "secure": secure,
                "httpOnly": http_only,
                "sameSite": match same_site { 0 => "no_restriction", 2 => "strict", _ => "lax" },
            });
            // ".site" cookies are for the subdomains too; the others only
            // for their host
            if host.starts_with('.') {
                c["domain"] = host.clone().into();
            }
            if let Some(e) = expiry {
                c["expirationDate"] = e.into();
            }
            out.push(c);
        }
    }
    drop(db);
    let _ = std::fs::remove_file(copy);
    Some(out)
}

fn read_logins(path: &Path, key: &[u8]) -> Option<Vec<Login>> {
    let (db, copy) = open_copy(path)?;
    let mut out = Vec::new();
    {
        let mut st = db
            .prepare(
                "SELECT origin_url, username_value, password_value FROM logins \
                 WHERE blacklisted_by_user = 0",
            )
            .ok()?;
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })
            .ok()?;
        for (url, username, enc) in rows.flatten() {
            let Some(password) = decrypt(key, &enc).and_then(|p| String::from_utf8(p).ok()) else {
                continue;
            };
            let Some(origin) = origin_of(&url) else { continue };
            if !password.is_empty() {
                out.push(Login {
                    origin,
                    username,
                    password,
                });
            }
        }
    }
    drop(db);
    let _ = std::fs::remove_file(copy);
    Some(out)
}

/// The localStorage of every site, from a copy of its LevelDB folder.
fn read_storage(dir: &Path) -> std::collections::HashMap<String, Vec<(String, String)>> {
    let copy = std::env::temp_dir().join(format!("idlerx-move-{}", crate::config::new_id()));
    if std::fs::create_dir_all(&copy).is_err() {
        return Default::default();
    }
    if let Ok(files) = std::fs::read_dir(dir) {
        for f in files.flatten() {
            let _ = std::fs::copy(f.path(), copy.join(f.file_name()));
        }
    }
    let out = crate::leveldb::local_storage(&crate::leveldb::read_dir(&copy));
    let _ = std::fs::remove_dir_all(&copy);
    out
}

/// "https://site:port" of an address.
fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| format!("{scheme}://{host}"))
}

/// Deletes the previous engine's folders (asked in Settings).
pub fn delete_old() {
    std::thread::spawn(|| {
        for dir in old_dirs() {
            for _ in 0..10 {
                if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_and_times() {
        assert_eq!(origin_of("https://game.example/login?x=1").as_deref(), Some("https://game.example"));
        assert_eq!(origin_of("http://a.b:8080/").as_deref(), Some("http://a.b:8080"));
        assert_eq!(origin_of("nonsense"), None);
        // 2024-01-01 00:00:00 UTC
        assert_eq!(unix_time(13_348_540_800_000_000), 1_704_067_200);
    }

    #[test]
    fn decrypts_chromium_values() {
        use aes_gcm::aead::{Aead, KeyInit};
        let key = [7u8; 32];
        let nonce = [1u8; 12];
        let cipher = aes_gcm::Aes256Gcm::new_from_slice(&key).unwrap();
        let ct = cipher
            .encrypt(aes_gcm::Nonce::from_slice(&nonce), b"segredo".as_slice())
            .unwrap();
        let data = [b"v10".as_slice(), &nonce, &ct].concat();
        assert_eq!(decrypt(&key, &data).unwrap(), b"segredo");
        assert!(decrypt(&key, b"v20abcdefghijklmnopqrstuvwxyz0123").is_none());
    }
}

#[cfg(test)]
mod real {
    /// Reads the accounts of a test data folder (IDLERX_DATA), printing
    /// only names and counts.
    #[test]
    #[ignore]
    fn read_test_profiles() {
        for p in std::env::var("PROFILES").unwrap().split(',') {
            match super::read(p) {
                Some(m) => println!(
                    "{p}: {} cookies {:?}, {} logins, storage {:?}",
                    m.cookies.len(),
                    m.cookies.iter().map(|c| format!("{}={}", c["name"].as_str().unwrap_or(""), c["value"].as_str().unwrap_or(""))).collect::<Vec<_>>(),
                    m.logins.len(),
                    m.storage.iter().map(|(o, items)| (o.clone(), items.iter().map(|(k, v)| format!("{k}:{} chars:{}", v.chars().count(), v.chars().take(14).collect::<String>())).collect::<Vec<_>>())).collect::<Vec<_>>()
                ),
                None => println!("{p}: nothing"),
            }
        }
    }
}
