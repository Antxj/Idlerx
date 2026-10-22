//! Saved passwords of each account, kept by Idlerx (Firefox's own password
//! manager is shared by every container of a profile, so it would offer one
//! account's logins in another). One file, `passwords.dat`, a JSON encrypted
//! by Windows (DPAPI, current user): only this Windows account can read it.
//! Keyed by the account's profile, so "clear data" starts it empty too.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Login {
    /// The site (scheme, host and port), e.g. `https://game.example`.
    pub origin: String,
    pub username: String,
    pub password: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Store {
    /// profile -> its logins
    logins: HashMap<String, Vec<Login>>,
    /// profile -> sites where saving is never offered
    never: HashMap<String, Vec<String>>,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn path() -> PathBuf {
    crate::config::data_dir().join("passwords.dat")
}

fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let store = guard.get_or_insert_with(|| {
        std::fs::read(path())
            .ok()
            .and_then(|d| crate::vpn::secrets::dpapi(&d, false))
            .and_then(|j| serde_json::from_slice(&j).ok())
            .unwrap_or_default()
    });
    f(store)
}

fn save(store: &Store) {
    let Ok(json) = serde_json::to_vec(store) else {
        return;
    };
    let Some(data) = crate::vpn::secrets::dpapi(&json, true) else {
        return;
    };
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, data).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

/// The account's logins for a site.
pub fn logins(profile: &str, origin: &str) -> Vec<Login> {
    with(|s| {
        s.logins
            .get(profile)
            .map(|l| l.iter().filter(|l| l.origin == origin).cloned().collect())
            .unwrap_or_default()
    })
}

/// Every login of the account.
pub fn list(profile: &str) -> Vec<Login> {
    with(|s| s.logins.get(profile).cloned().unwrap_or_default())
}

pub fn is_saved(profile: &str, offer: &crate::firefox::LoginOffer) -> bool {
    logins(profile, &offer.origin)
        .iter()
        .any(|l| l.username == offer.username && l.password == offer.password)
}

pub fn never(profile: &str, origin: &str) -> bool {
    with(|s| {
        s.never
            .get(profile)
            .is_some_and(|n| n.iter().any(|o| o == origin))
    })
}

/// Saves a login (replacing the same user name on the same site).
pub fn store(profile: &str, login: Login) {
    with(|s| {
        let list = s.logins.entry(profile.to_string()).or_default();
        list.retain(|l| !(l.origin == login.origin && l.username == login.username));
        list.push(login);
        save(s);
    });
}

pub fn set_never(profile: &str, origin: &str) {
    with(|s| {
        let n = s.never.entry(profile.to_string()).or_default();
        if !n.iter().any(|o| o == origin) {
            n.push(origin.to_string());
        }
        save(s);
    });
}

pub fn remove(profile: &str, origin: &str, username: &str) {
    with(|s| {
        if let Some(list) = s.logins.get_mut(profile) {
            list.retain(|l| !(l.origin == origin && l.username == username));
        }
        save(s);
    });
}

/// The account was deleted or its data cleared.
pub fn forget(profile: &str) {
    with(|s| {
        let had = s.logins.remove(profile).is_some() | s.never.remove(profile).is_some();
        if had {
            save(s);
        }
    });
}

/// Logins brought from elsewhere (the previous engine), without replacing
/// what the account already has.
pub fn import(profile: &str, logins: Vec<Login>) -> usize {
    with(|s| {
        let list = s.logins.entry(profile.to_string()).or_default();
        let mut added = 0;
        for l in logins {
            if !list
                .iter()
                .any(|x| x.origin == l.origin && x.username == l.username)
            {
                list.push(l);
                added += 1;
            }
        }
        if added > 0 {
            save(s);
        }
        added
    })
}
