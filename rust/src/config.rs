//! Accounts and settings, saved as JSON in `%APPDATA%\Idlerx\config.json`.
//!
//! Each account has its own browser profile (cookies, cache, storage) in
//! `%LOCALAPPDATA%\Idlerx\profiles\<profile>`. The profile name is separate
//! from the account id so "clear data" can switch to a fresh profile at once
//! while the old folder is deleted in the background.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Account {
    pub id: String,
    /// Browser profile folder name.
    pub profile: String,
    pub name: String,
    /// Start address.
    pub url: String,
    /// Last visited address, reopened on the next start.
    pub last_url: Option<String>,
    pub favorite: bool,
    pub muted: bool,
    pub zoom: f64,
    /// Shown in the grid layout.
    pub in_grid: bool,
    /// Open (running) when the app was closed.
    pub open: bool,
}

impl Default for Account {
    fn default() -> Self {
        Account {
            id: String::new(),
            profile: String::new(),
            name: String::new(),
            url: String::new(),
            last_url: None,
            favorite: false,
            muted: false,
            zoom: 1.0,
            in_grid: true,
            open: false,
        }
    }
}

impl Account {
    pub fn new(name: String, url: String) -> Account {
        let id = new_id();
        Account {
            profile: id.clone(),
            id,
            name,
            url,
            ..Account::default()
        }
    }

    /// Address to open: the last visited one, or the start address.
    pub fn start_url(&self) -> &str {
        self.last_url
            .as_deref()
            .filter(|u| u.starts_with("http"))
            .unwrap_or(&self.url)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// One account at a time.
    #[default]
    Tabs,
    /// Every open account side by side.
    Grid,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub accounts: Vec<Account>,
    /// "pt", "en" or None (Windows language).
    pub language: Option<String>,
    pub theme: Theme,
    pub layout: Layout,
    /// Grid columns; 0 = automatic.
    pub grid_columns: u8,
    pub selected: Option<String>,
    pub check_updates: bool,
    pub restore_open: bool,
    /// Keeps hidden pages running at full speed (no background throttling).
    pub keep_active: bool,
    pub window_size: Option<[f32; 2]>,
    pub window_maximized: bool,
    /// Profile folders still to be deleted (they were in use).
    pub pending_delete: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            accounts: Vec::new(),
            language: None,
            theme: Theme::System,
            layout: Layout::Tabs,
            grid_columns: 0,
            selected: None,
            check_updates: true,
            restore_open: true,
            keep_active: true,
            window_size: None,
            window_maximized: false,
            pending_delete: Vec::new(),
        }
    }
}

impl Config {
    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn account_mut(&mut self, id: &str) -> Option<&mut Account> {
        self.accounts.iter_mut().find(|a| a.id == id)
    }

    /// Display order: favorites first, then the others, each group in the
    /// saved order.
    pub fn display_order(&self) -> Vec<String> {
        let fav = self.accounts.iter().filter(|a| a.favorite);
        let rest = self.accounts.iter().filter(|a| !a.favorite);
        fav.chain(rest).map(|a| a.id.clone()).collect()
    }

    /// Moves `id` to the position of `target` (drag and drop).
    pub fn move_to(&mut self, id: &str, target: &str) {
        if id == target {
            return;
        }
        let Some(from) = self.accounts.iter().position(|a| a.id == id) else {
            return;
        };
        let Some(to) = self.accounts.iter().position(|a| a.id == target) else {
            return;
        };
        let acc = self.accounts.remove(from);
        self.accounts.insert(to, acc);
    }
}

// ------------------------------------------------------------------ paths --

/// IDLERX_DATA (tests and screenshots) puts everything in another folder so
/// the real accounts are not touched.
fn override_dir() -> Option<PathBuf> {
    std::env::var_os("IDLERX_DATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Settings folder (%APPDATA%\Idlerx).
pub fn data_dir() -> PathBuf {
    override_dir().unwrap_or_else(|| {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Idlerx")
    })
}

/// Browser profiles folder (%LOCALAPPDATA%\Idlerx\profiles): caches are
/// large and do not belong in the roaming profile.
pub fn profiles_dir() -> PathBuf {
    match override_dir() {
        Some(d) => d.join("profiles"),
        None => std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(data_dir)
            .join("Idlerx")
            .join("profiles"),
    }
}

pub fn profile_dir(profile: &str) -> PathBuf {
    profiles_dir().join(profile)
}

fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

// ------------------------------------------------------------ persistence --

/// Reads the settings. A damaged file is kept aside as `config.json.bad`
/// (never silently overwritten) and the app starts empty.
pub fn load() -> Config {
    let path = config_path();
    let Ok(bytes) = std::fs::read(&path) else {
        return Config::default();
    };
    // a file saved by Notepad may start with a UTF-8 BOM
    let json = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    match serde_json::from_slice::<Config>(json) {
        Ok(mut c) => {
            // accounts saved without a profile (hand-edited file)
            for a in &mut c.accounts {
                if a.id.is_empty() {
                    a.id = new_id();
                }
                if a.profile.is_empty() {
                    a.profile = a.id.clone();
                }
            }
            c
        }
        Err(_) => {
            let _ = std::fs::rename(&path, path.with_extension("json.bad"));
            Config::default()
        }
    }
}

/// Saves atomically: writes a temporary file and then renames it.
pub fn save(cfg: &Config) -> std::io::Result<()> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join("config.json.tmp");
    let json = serde_json::to_vec_pretty(cfg).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, config_path())
}

/// Deletes the profile folders waiting for deletion; keeps those still in use.
pub fn delete_pending(cfg: &mut Config) {
    cfg.pending_delete.retain(|p| {
        let dir = profile_dir(p);
        dir.exists() && std::fs::remove_dir_all(&dir).is_err()
    });
}

/// Deletes a profile folder in the background: the browser of a closed
/// account takes a moment to release the files.
pub fn delete_profile_later(profile: String) {
    std::thread::spawn(move || {
        let dir = profile_dir(&profile);
        for _ in 0..20 {
            if !dir.exists() || std::fs::remove_dir_all(&dir).is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

// ---------------------------------------------------------------- helpers --

/// Random id (16 hex characters).
pub fn new_id() -> String {
    let mut b = [0u8; 8];
    if getrandom::fill(&mut b).is_err() {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        b = (t as u64 ^ std::process::id() as u64).to_le_bytes();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// What the user typed -> address to open. Without a scheme, https:// is
/// assumed. Only http and https are accepted.
pub fn normalize_url(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    let full = if lower.starts_with("https://") || lower.starts_with("http://") {
        s.to_string()
    } else if s.contains("://") || lower.starts_with("javascript:") || lower.starts_with("data:") {
        return None;
    } else {
        format!("https://{s}")
    };
    let host = host_of(&full);
    let ok = !host.is_empty()
        && (host.contains('.') || host.eq_ignore_ascii_case("localhost") || host.contains(':'));
    ok.then_some(full)
}

/// Host part of an address ("https://www.site.com/x" -> "www.site.com").
pub fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..end];
    // user:password@host
    host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host)
}

/// Short domain for display ("www.site.com:8080" -> "site.com:8080").
pub fn domain(url: &str) -> &str {
    let h = host_of(url);
    h.strip_prefix("www.").unwrap_or(h)
}

/// Suggested account name for an address ("www.tibia.com" -> "Tibia").
pub fn name_from_url(url: &str) -> String {
    let d = domain(url);
    let d = d.split(':').next().unwrap_or(d);
    let parts: Vec<&str> = d.split('.').collect();
    let main = if parts.len() >= 2 {
        parts[parts.len() - 2]
    } else {
        d
    };
    let mut c = main.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Up to two initials for the avatar.
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_')
        .filter(|w| !w.is_empty())
        .collect();
    let s: String = match words.as_slice() {
        [] => "?".into(),
        [one] => one
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(2)
            .collect(),
        [a, b, ..] => a.chars().take(1).chain(b.chars().take(1)).collect(),
    };
    if s.is_empty() {
        "?".into()
    } else {
        s.to_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enderecos() {
        assert_eq!(
            normalize_url("tibia.com").as_deref(),
            Some("https://tibia.com")
        );
        assert_eq!(
            normalize_url("  https://www.site.com/game?x=1 ").as_deref(),
            Some("https://www.site.com/game?x=1")
        );
        assert_eq!(
            normalize_url("http://localhost:8080").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(
            normalize_url("localhost:3000").as_deref(),
            Some("https://localhost:3000")
        );
        assert_eq!(normalize_url(""), None);
        assert_eq!(normalize_url("not a url"), None);
        assert_eq!(normalize_url("javascript:alert(1)"), None);
        assert_eq!(normalize_url("file:///C:/x"), None);
        assert_eq!(normalize_url("palavra"), None);
    }

    #[test]
    fn dominios_e_nomes() {
        assert_eq!(host_of("https://user:pw@www.a.com:81/x"), "www.a.com:81");
        assert_eq!(
            domain("https://www.grinderscape.org/game"),
            "grinderscape.org"
        );
        assert_eq!(name_from_url("https://www.tibia.com/news"), "Tibia");
        assert_eq!(
            name_from_url("https://play.otuniverse.online"),
            "Otuniverse"
        );
        assert_eq!(initials("Char Rubinaum"), "CR");
        assert_eq!(initials("GrinderAlt"), "GR");
        assert_eq!(initials("OT-Universe #2"), "OU");
        assert_eq!(initials("  "), "?");
    }

    #[test]
    fn ordem_e_arrastar() {
        let mut c = Config::default();
        for n in ["a", "b", "c", "d"] {
            let mut a = Account::new(n.into(), "https://x.com".into());
            a.id = n.into();
            c.accounts.push(a);
        }
        c.accounts[2].favorite = true;
        assert_eq!(c.display_order(), ["c", "a", "b", "d"]);
        c.move_to("d", "a");
        assert_eq!(c.display_order(), ["c", "d", "a", "b"]);
        // moving down: takes the target's place (index 2)
        c.move_to("d", "b");
        let ids: Vec<_> = c.accounts.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "d", "c"]);
    }

    #[test]
    fn json_antigo_ou_parcial() {
        let c: Config =
            serde_json::from_str(r#"{"accounts":[{"id":"x","name":"N","url":"https://a.com"}]}"#)
                .unwrap();
        assert_eq!(c.accounts[0].zoom, 1.0);
        assert!(c.accounts[0].in_grid);
        assert!(c.check_updates && c.restore_open && c.keep_active);
        let a = &c.accounts[0];
        assert_eq!(a.start_url(), "https://a.com");
    }

    #[test]
    fn ids_unicos() {
        let a = new_id();
        assert_eq!(a.len(), 16);
        assert_ne!(a, new_id());
    }
}
