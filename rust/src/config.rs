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
    /// Group in the side list (None = no group).
    pub group: Option<String>,
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
            group: None,
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

/// Group of accounts in the side list.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub id: String,
    pub name: String,
    /// Accounts hidden in the side list.
    pub collapsed: bool,
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
    pub groups: Vec<Group>,
    /// "pt", "en" or None (Windows language).
    pub language: Option<String>,
    pub theme: Theme,
    pub layout: Layout,
    /// Grid columns; 0 = automatic (columns and rows).
    pub grid_cols: u8,
    /// Grid rows; 0 = as many as needed. Accounts that do not fit in a
    /// fixed grid keep running, only off screen.
    pub grid_rows: u8,
    /// Grid shows only this group (None = all accounts).
    pub grid_group: Option<String>,
    pub selected: Option<String>,
    pub check_updates: bool,
    pub restore_open: bool,
    /// Keeps hidden pages running at full speed (no background throttling).
    pub keep_active: bool,
    /// Economy mode: one shared browser, one profile per account.
    pub economy: bool,
    pub window_size: Option<[f32; 2]>,
    pub window_maximized: bool,
    /// Profile folders still to be deleted (they were in use).
    pub pending_delete: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            accounts: Vec::new(),
            groups: Vec::new(),
            language: None,
            theme: Theme::System,
            layout: Layout::Tabs,
            grid_cols: 0,
            grid_rows: 0,
            grid_group: None,
            selected: None,
            check_updates: true,
            restore_open: true,
            keep_active: true,
            economy: true,
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

    pub fn group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_mut(&mut self, id: &str) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// Group of an account, ignoring groups that no longer exist.
    pub fn group_of<'a>(&self, a: &'a Account) -> Option<&'a str> {
        a.group.as_deref().filter(|g| self.group(g).is_some())
    }

    /// Accounts of a group (None = without group), in the saved order.
    pub fn accounts_in(&self, group: Option<&str>) -> Vec<String> {
        self.accounts
            .iter()
            .filter(|a| self.group_of(a) == group)
            .map(|a| a.id.clone())
            .collect()
    }

    /// Order of the side list (and of the grid): accounts without a group,
    /// then each group in order.
    pub fn display_order(&self) -> Vec<String> {
        let mut out = self.accounts_in(None);
        for g in &self.groups {
            out.extend(self.accounts_in(Some(&g.id)));
        }
        out
    }

    /// Moves `id` to the position of `target` (drag and drop) and into its
    /// group.
    pub fn move_to(&mut self, id: &str, target: &str) {
        if id == target {
            return;
        }
        let group = self
            .account(target)
            .and_then(|t| self.group_of(t).map(String::from));
        let Some(from) = self.accounts.iter().position(|a| a.id == id) else {
            return;
        };
        let Some(to) = self.accounts.iter().position(|a| a.id == target) else {
            return;
        };
        let mut acc = self.accounts.remove(from);
        acc.group = group;
        self.accounts.insert(to, acc);
    }

    /// Puts `id` at the end of a group (None = without group).
    pub fn move_to_group(&mut self, id: &str, group: Option<&str>) {
        let Some(from) = self.accounts.iter().position(|a| a.id == id) else {
            return;
        };
        let mut acc = self.accounts.remove(from);
        acc.group = group.filter(|g| self.group(g).is_some()).map(String::from);
        self.accounts.push(acc);
    }

    /// Puts `id` right after `target` (an empty grid cell after it).
    pub fn move_after(&mut self, id: &str, target: &str) {
        if id == target {
            return;
        }
        let group = self
            .account(target)
            .and_then(|t| self.group_of(t).map(String::from));
        let Some(from) = self.accounts.iter().position(|a| a.id == id) else {
            return;
        };
        let mut acc = self.accounts.remove(from);
        acc.group = group;
        let to = self
            .accounts
            .iter()
            .position(|a| a.id == target)
            .map_or(self.accounts.len(), |p| p + 1);
        self.accounts.insert(to, acc);
    }

    /// Removes a group; its accounts stay, without group.
    pub fn delete_group(&mut self, id: &str) {
        self.groups.retain(|g| g.id != id);
        for a in &mut self.accounts {
            if a.group.as_deref() == Some(id) {
                a.group = None;
            }
        }
        if self.grid_group.as_deref() == Some(id) {
            self.grid_group = None;
        }
    }

    /// Accounts shown in the grid (given which ones are open), in order.
    pub fn grid_accounts(&self, is_open: impl Fn(&str) -> bool) -> Vec<String> {
        let only = self
            .grid_group
            .as_deref()
            .filter(|g| self.group(g).is_some());
        self.display_order()
            .into_iter()
            .filter(|id| {
                self.account(id)
                    .is_some_and(|a| a.in_grid && (only.is_none() || self.group_of(a) == only))
                    && is_open(id)
            })
            .collect()
    }
}

/// Grid size for `n` accounts: (columns, rows). `auto` picks the columns
/// when they are automatic.
pub fn grid_shape(cols: u8, rows: u8, n: usize, auto: impl Fn(usize) -> usize) -> (usize, usize) {
    let n = n.max(1);
    match (cols as usize, rows as usize) {
        (0, _) => {
            let c = auto(n).clamp(1, n);
            (c, n.div_ceil(c))
        }
        (c, 0) => {
            let c = c.min(n);
            (c, n.div_ceil(c))
        }
        (c, r) => (c, r),
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

/// Browser folder of economy mode (one profile per account inside it).
pub fn shared_dir() -> PathBuf {
    profiles_dir().join("_shared")
}

/// Every place an account's browser data can be: its own folder and its
/// profile inside the shared browser (economy mode).
fn profile_paths(profile: &str) -> [PathBuf; 2] {
    [
        profile_dir(profile),
        shared_dir().join("EBWebView").join(profile),
    ]
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
        profile_paths(p)
            .iter()
            .any(|dir| dir.exists() && std::fs::remove_dir_all(dir).is_err())
    });
}

/// Deletes a profile folder in the background: the browser of a closed
/// account takes a moment to release the files.
pub fn delete_profile_later(profile: String) {
    std::thread::spawn(move || {
        let dirs = profile_paths(&profile);
        for _ in 0..20 {
            let gone = dirs
                .iter()
                .all(|dir| !dir.exists() || std::fs::remove_dir_all(dir).is_ok());
            if gone {
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

    fn conta(c: &mut Config, id: &str, group: Option<&str>) {
        let mut a = Account::new(id.into(), "https://x.com".into());
        a.id = id.into();
        a.group = group.map(String::from);
        c.accounts.push(a);
    }

    fn grupo(c: &mut Config, id: &str) {
        c.groups.push(Group {
            id: id.into(),
            name: id.into(),
            collapsed: false,
        });
    }

    #[test]
    fn ordem_e_arrastar() {
        let mut c = Config::default();
        for n in ["a", "b", "c", "d"] {
            conta(&mut c, n, None);
        }
        // favorites no longer jump to the top: the order is the user's
        c.accounts[2].favorite = true;
        assert_eq!(c.display_order(), ["a", "b", "c", "d"]);
        c.move_to("d", "a");
        assert_eq!(c.display_order(), ["d", "a", "b", "c"]);
        // moving down: takes the target's place (after it)
        c.move_to("d", "b");
        assert_eq!(c.display_order(), ["a", "b", "d", "c"]);
        c.move_after("a", "c");
        assert_eq!(c.display_order(), ["b", "d", "c", "a"]);
    }

    #[test]
    fn grupos() {
        let mut c = Config::default();
        grupo(&mut c, "g1");
        grupo(&mut c, "g2");
        conta(&mut c, "a", Some("g2"));
        conta(&mut c, "b", None);
        conta(&mut c, "c", Some("g1"));
        conta(&mut c, "d", Some("sumiu")); // group that does not exist
        assert_eq!(c.display_order(), ["b", "d", "c", "a"]);
        assert_eq!(c.accounts_in(Some("g1")), ["c"]);

        // dropping onto an account of another group moves it into that group
        c.move_to("b", "a");
        assert_eq!(c.account("b").unwrap().group.as_deref(), Some("g2"));
        assert_eq!(c.display_order(), ["d", "c", "b", "a"]);

        c.move_to_group("d", Some("g1"));
        assert_eq!(c.display_order(), ["c", "d", "b", "a"]);
        c.move_to_group("c", Some("nao-existe"));
        assert_eq!(c.account("c").unwrap().group, None);

        c.grid_group = Some("g2".into());
        assert_eq!(c.grid_accounts(|_| true), ["b", "a"]);
        assert_eq!(c.grid_accounts(|id| id != "a"), ["b"]);
        c.delete_group("g2");
        assert_eq!(c.grid_group, None);
        assert!(c.accounts.iter().all(|a| a.group.as_deref() != Some("g2")));
    }

    #[test]
    fn formato_da_grade() {
        let auto = |n: usize| if n <= 2 { n } else { 2 };
        assert_eq!(grid_shape(0, 0, 4, auto), (2, 2));
        assert_eq!(grid_shape(0, 0, 1, auto), (1, 1));
        assert_eq!(grid_shape(0, 0, 0, auto), (1, 1));
        // fixed columns, as many rows as needed
        assert_eq!(grid_shape(3, 0, 7, auto), (3, 3));
        assert_eq!(grid_shape(3, 0, 2, auto), (2, 1));
        // fixed columns and rows: always that size
        assert_eq!(grid_shape(2, 2, 1, auto), (2, 2));
        assert_eq!(grid_shape(2, 2, 9, auto), (2, 2));
    }

    #[test]
    fn json_antigo_ou_parcial() {
        let c: Config =
            serde_json::from_str(r#"{"accounts":[{"id":"x","name":"N","url":"https://a.com"}]}"#)
                .unwrap();
        assert_eq!(c.accounts[0].zoom, 1.0);
        assert!(c.accounts[0].in_grid);
        assert!(c.check_updates && c.restore_open && c.keep_active && c.economy);
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
