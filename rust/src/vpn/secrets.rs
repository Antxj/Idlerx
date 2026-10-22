//! VPN passwords and token seeds, one entry per group, kept apart from
//! config.json in `vpn\secrets.dat`: a JSON encrypted by Windows (DPAPI,
//! current user scope) - only this Windows account can read it.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use windows_sys::Win32::Foundation::{LocalFree, HLOCAL};
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Secret {
    pub password: String,
    /// TOTP seed (base32), for tokens like Google Authenticator.
    pub seed: String,
}

fn path() -> PathBuf {
    super::dir().join("secrets.dat")
}

pub(crate) fn dpapi(data: &[u8], protect: bool) -> Option<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    use std::ptr::{null, null_mut};
    let ok = unsafe {
        if protect {
            CryptProtectData(&input, null(), null(), null(), null(), 0, &mut output)
        } else {
            CryptUnprotectData(&input, null_mut(), null(), null(), null(), 0, &mut output)
        }
    };
    if ok == 0 || output.pbData.is_null() {
        return None;
    }
    let out = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe { LocalFree(output.pbData as HLOCAL) };
    Some(out)
}

fn load_all() -> HashMap<String, Secret> {
    std::fs::read(path())
        .ok()
        .and_then(|data| dpapi(&data, false))
        .and_then(|json| serde_json::from_slice(&json).ok())
        .unwrap_or_default()
}

fn save_all(all: &HashMap<String, Secret>) -> std::io::Result<()> {
    let json = serde_json::to_vec(all)?;
    let data = dpapi(&json, true).ok_or_else(|| std::io::Error::other("DPAPI"))?;
    std::fs::create_dir_all(super::dir())?;
    let tmp = path().with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(tmp, path())
}

/// Secret of a group (empty when there is none).
pub fn get(group: &str) -> Secret {
    load_all().remove(group).unwrap_or_default()
}

pub fn set(group: &str, secret: Secret) -> std::io::Result<()> {
    let mut all = load_all();
    if secret == Secret::default() {
        all.remove(group);
    } else {
        all.insert(group.to_string(), secret);
    }
    save_all(&all)
}

pub fn remove(group: &str) {
    let mut all = load_all();
    if all.remove(group).is_some() {
        let _ = save_all(&all);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_round_trip() {
        let data = b"senha secreta";
        let enc = dpapi(data, true).unwrap();
        assert_ne!(&enc[..], data);
        assert_eq!(dpapi(&enc, false).unwrap(), data);
        assert!(dpapi(b"lixo", false).is_none());
    }
}
