//! Built-in ad and tracker blocker, with Brave's engine (adblock-rust) and
//! the lists uBlock Origin uses by default (uBlock filters, EasyList,
//! EasyPrivacy, Peter Lowe's list).
//!
//! Browser extensions cannot do it inside Idlerx: WebView2 runs them, but
//! the pages' requests never reach them. Instead every request of every
//! page is checked here (see `panels`, WebResourceRequested) and the ones
//! the lists block get an empty answer; the page also gets a style hiding
//! the spots of the ads (cosmetic filters).
//!
//! As in uBlock Origin, the lists can also run small scripts in a page
//! (`+js(...)` rules: they defuse pop-unders, anti-blocker checks, ad
//! timers) and answer some blocked requests with a harmless stand-in
//! (`$redirect`, so a game that loads an ad library keeps working). Both
//! come from uBlock Origin itself (`assets/ubo-resources.json`, made by
//! `tools/ubo_resources.mjs`).
//!
//! The user's own rules (hidden elements, see `element_rule`) join the
//! lists.
//!
//! The lists are downloaded into the data folder and refreshed once a day
//! in the background; the engine is rebuilt off the interface thread and
//! swapped in when ready.

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::resources::{PermissionMask, Resource};
use adblock::Engine;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, SystemTime};

/// (file name, address) of each list.
const LISTS: &[(&str, &str)] = &[
    ("ublock-filters", "https://ublockorigin.github.io/uAssets/filters/filters.min.txt"),
    ("ublock-badware", "https://ublockorigin.github.io/uAssets/filters/badware.min.txt"),
    ("ublock-privacy", "https://ublockorigin.github.io/uAssets/filters/privacy.min.txt"),
    ("ublock-unbreak", "https://ublockorigin.github.io/uAssets/filters/unbreak.min.txt"),
    ("ublock-quick-fixes", "https://ublockorigin.github.io/uAssets/filters/quick-fixes.min.txt"),
    ("easylist", "https://easylist.to/easylist/easylist.txt"),
    ("easyprivacy", "https://easylist.to/easylist/easyprivacy.txt"),
    (
        "peter-lowe",
        "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=adblockplus&showintro=1&mimetype=plaintext",
    ),
];
const REFRESH: Duration = Duration::from_secs(24 * 60 * 60);
/// Look at the clock this often (a computer that slept does not delay it).
const TICK: Duration = Duration::from_secs(10 * 60);
const LIST_LIMIT: usize = 16 * 1024 * 1024;

/// uBlock Origin's scriptlets and stand-ins (see the top of the file).
const UBO_RESOURCES: &str = include_str!("../assets/ubo-resources.json");
/// What uBlock calls "trusted" scriptlets (they can set cookies, for one):
/// only uBlock's own lists and the user's rules may use them.
const TRUSTED: PermissionMask = PermissionMask::from_bits(1);

/// State shown in the settings.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// Rules in the engine in use (0 = not ready yet).
    pub rules: usize,
    /// When the lists were last downloaded.
    pub updated: Option<SystemTime>,
    pub updating: bool,
    pub error: Option<String>,
}

/// Pop-up rules of the lists (`$popup`), which the engine does not handle:
/// ad networks by domain, and address fragments (with `*` wildcards).
#[derive(Default)]
struct PopupRules {
    domains: std::collections::HashSet<String>,
    fragments: Vec<Vec<String>>,
}

impl PopupRules {
    fn parse(text: &str, into: &mut PopupRules) {
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('!') || line.starts_with("@@") {
                continue;
            }
            let Some((pattern, options)) = line.rsplit_once('$') else {
                continue;
            };
            let opts: Vec<&str> = options.split(',').collect();
            // only plain pop-up rules: not limited to some sites
            if !opts.contains(&"popup") || opts.iter().any(|o| o.starts_with("domain=")) {
                continue;
            }
            if let Some(host) = pattern.strip_prefix("||").and_then(|p| p.strip_suffix('^')) {
                if !host.is_empty()
                    && host
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
                {
                    into.domains.insert(host.to_ascii_lowercase());
                }
            } else if !pattern.is_empty() && !pattern.contains(['^', '|']) && pattern.len() > 4 {
                let parts: Vec<String> = pattern
                    .split('*')
                    .filter(|p| !p.is_empty())
                    .map(String::from)
                    .collect();
                if !parts.is_empty() {
                    into.fragments.push(parts);
                }
            }
        }
    }

    fn matches(&self, url: &str) -> bool {
        let host = host(url).to_ascii_lowercase();
        let mut h = host.as_str();
        loop {
            if self.domains.contains(h) {
                return true;
            }
            match h.split_once('.') {
                Some((_, rest)) if rest.contains('.') => h = rest,
                _ => break,
            }
        }
        self.fragments.iter().any(|parts| {
            let mut from = 0;
            parts.iter().all(|p| match url[from..].find(p.as_str()) {
                Some(i) => {
                    from += i + p.len();
                    true
                }
                None => false,
            })
        })
    }
}

#[derive(Default)]
struct Shared {
    engine: RwLock<Option<Arc<Engine>>>,
    popups: RwLock<Arc<PopupRules>>,
    status: Mutex<Status>,
    /// Requests blocked since the app opened.
    blocked: AtomicU64,
    /// "Update now" from the settings.
    refresh_now: std::sync::atomic::AtomicBool,
    /// The user's rules changed: the engine is built again.
    rebuild: std::sync::atomic::AtomicBool,
}

static SHARED: OnceLock<Shared> = OnceLock::new();

/// Blocking on (setting) and the sites where it is off, set by the app.
static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static ALLOW: RwLock<Vec<String>> = RwLock::new(Vec::new());
/// The user's own rules (`site##selector`), set by the app.
static RULES: RwLock<Vec<String>> = RwLock::new(Vec::new());

pub fn configure(enabled: bool, allow: &[String], rules: &[String]) {
    ENABLED.store(enabled, Ordering::Relaxed);
    *ALLOW.write().unwrap_or_else(|e| e.into_inner()) = allow.to_vec();
    let mut r = RULES.write().unwrap_or_else(|e| e.into_inner());
    if *r != rules {
        *r = rules.to_vec();
        shared().rebuild.store(true, Ordering::Relaxed);
    }
}

/// The lists are loaded (blocking works).
pub fn ready() -> bool {
    engine().is_some()
}

/// The lists are on the disk but still loading (the first seconds after
/// the app starts): pages opened now would see no blocking at all.
pub fn warming_up() -> bool {
    enabled() && engine().is_none() && newest_download().is_some()
}

/// Nothing is blocked on the site of `url` (it is in the allowed list).
pub fn site_allowed(url: &str) -> bool {
    allowed(url, &allow_list())
}

/// Host of an address without its port, in lower case ("https://A.b:8080/x"
/// -> "a.b"), as pages see it (`location.hostname`) and rules name it.
pub fn hostname(url: &str) -> String {
    let h = host(url);
    let h = match h.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => h,
    };
    h.to_ascii_lowercase()
}

/// A rule that hides an element of a site, as the user picked it:
/// `host##selector`.
pub fn element_rule(url: &str, selector: &str) -> Option<String> {
    let host = hostname(url);
    let selector = selector.trim();
    (url.starts_with("http")
        && !host.is_empty()
        && !selector.is_empty()
        && !selector.contains(['\n', '\r']))
    .then(|| format!("{host}##{selector}"))
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn allow_list() -> Vec<String> {
    ALLOW.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// What to do with a request.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Allow,
    /// An empty answer.
    Block,
    /// A harmless stand-in (a `data:` address with its content).
    Redirect(String),
}

/// `verdict` with the app's settings.
pub fn check(url: &str, source: &str, kind: Kind) -> Verdict {
    if !enabled() {
        return Verdict::Allow;
    }
    verdict(url, source, kind, &allow_list())
}

/// The scripts the lists run in pages of `url` (uBlock's `+js(...)`
/// rules), as one script; None when there are none.
pub fn scriptlets_for(url: &str) -> Option<String> {
    if !enabled() || !url.starts_with("http") || allowed(url, &allow_list()) {
        return None;
    }
    let script = engine()?.url_cosmetic_resources(url).injected_script;
    (!script.trim().is_empty()).then_some(script)
}

/// A page wants to open a new window: whether it is an ad (pop-up rules of
/// the lists, an ad server, or opened without the user clicking). Counts it
/// when so.
pub fn popup_blocked(target: &str, source: &str, user_initiated: bool) -> bool {
    if !enabled() || allowed(source, &allow_list()) {
        return false;
    }
    if popup_ad(target, source) {
        return true;
    }
    // a window nobody asked for is never wanted in a game
    if !user_initiated {
        shared().blocked.fetch_add(1, Ordering::Relaxed);
    }
    !user_initiated
}

/// A pop-up window (opened by the page `source`) goes to `target`: whether
/// that is an ad. Checked on each address the window loads, since pop-ups
/// often open blank and only then go to the ad. Counts it when so.
pub fn popup_navigation_blocked(target: &str, source: &str) -> bool {
    enabled() && !allowed(source, &allow_list()) && popup_ad(target, source)
}

fn popup_ad(target: &str, source: &str) -> bool {
    if !target.starts_with("http") {
        return false;
    }
    let rules = shared()
        .popups
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    // pop-up networks hide the ad's address inside their own, on domains
    // that change all the time: those addresses are checked too
    let mut urls = vec![target.to_string()];
    urls.extend(embedded_urls(target));
    if urls
        .iter()
        .any(|u| u.starts_with("http") && rules.matches(u))
    {
        shared().blocked.fetch_add(1, Ordering::Relaxed);
        return true;
    }
    // the engine counts what it blocks itself
    urls.iter()
        .any(|u| should_block(u, source, Kind::Subdocument, &allow_list()))
}

/// Addresses carried inside an address: as a parameter (`?url=https%3A…`)
/// or a path piece encoded in base64 (`/_x/aHR0cHM6Ly…`), two levels deep.
fn embedded_urls(url: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut todo = vec![url.to_string()];
    for _ in 0..2 {
        let mut next = Vec::new();
        for u in &todo {
            let rest = u.split_once("://").map_or(u.as_str(), |(_, r)| r);
            for piece in rest.split(['/', '?', '&', '=', '#']).skip(1) {
                let text = percent_decode(piece);
                let inner = if text.starts_with("http://") || text.starts_with("https://") {
                    Some(text)
                } else if piece.len() >= 16 {
                    base64_decode(piece).filter(|t| t.starts_with("http"))
                } else {
                    None
                };
                if let Some(inner) = inner.filter(|i| !found.contains(i)) {
                    found.push(inner.clone());
                    next.push(inner);
                }
            }
        }
        todo = next;
    }
    found
}

fn percent_decode(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (
            b[i],
            b.get(i + 1).and_then(|c| hex(*c)),
            b.get(i + 2).and_then(|c| hex(*c)),
        ) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Base64 (standard or URL-safe, padding optional) into text; None if it
/// is not base64 or not text.
fn base64_decode(text: &str) -> Option<String> {
    String::from_utf8(base64_bytes(text)?).ok()
}

/// A `data:` address (`data:mime;base64,...`): its type and content.
pub fn data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let (head, body) = url.strip_prefix("data:")?.split_once(',')?;
    let mime = head.strip_suffix(";base64")?;
    Some((mime.to_string(), base64_bytes(body)?))
}

/// Base64 (standard or URL-safe, padding optional) into bytes.
fn base64_bytes(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// `cosmetic_css` with the app's settings.
pub fn css_for(url: &str, classes: &[String], ids: &[String]) -> Option<String> {
    if !enabled() {
        return None;
    }
    cosmetic_css(url, classes, ids, &allow_list())
}

fn shared() -> &'static Shared {
    SHARED.get_or_init(Shared::default)
}

fn dir() -> PathBuf {
    crate::config::data_dir().join("adblock")
}

fn set_status(f: impl FnOnce(&mut Status)) {
    f(&mut shared().status.lock().unwrap_or_else(|e| e.into_inner()));
}

pub fn status() -> Status {
    shared()
        .status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn blocked() -> u64 {
    shared().blocked.load(Ordering::Relaxed)
}

pub fn refresh_now() {
    shared().refresh_now.store(true, Ordering::Relaxed);
}

fn engine() -> Option<Arc<Engine>> {
    shared()
        .engine
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Loads the saved lists (or downloads them) and keeps them fresh, in a
/// thread of its own. `wake` repaints the interface (status changes).
pub fn start(wake: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("adblock".into())
        .spawn(move || {
            let _ = std::fs::create_dir_all(dir());
            let mut last = newest_download();
            loop {
                // turned off: nothing is downloaded or kept in memory
                if !enabled() {
                    *shared().engine.write().unwrap_or_else(|e| e.into_inner()) = None;
                    set_status(|s| s.rules = 0);
                    std::thread::sleep(Duration::from_secs(5));
                    continue;
                }
                // what was downloaded before: blocking starts right away;
                // built again when the user's rules change
                let rebuild = shared().rebuild.swap(false, Ordering::Relaxed);
                if (engine().is_none() || rebuild) && last.is_some() {
                    build(&wake);
                }
                let due = last
                    .and_then(|t| t.elapsed().ok())
                    .is_none_or(|age| age >= REFRESH);
                if due || shared().refresh_now.swap(false, Ordering::Relaxed) {
                    set_status(|s| s.updating = true);
                    wake();
                    match download() {
                        Ok(()) => {
                            last = Some(SystemTime::now());
                            set_status(|s| s.error = None);
                            build(&wake);
                        }
                        Err(e) => set_status(|s| s.error = Some(e)),
                    }
                    set_status(|s| {
                        s.updating = false;
                        s.updated = newest_download();
                    });
                    wake();
                }
                // short naps, to notice "update now" or "off" soon
                for _ in 0..TICK.as_secs() / 5 {
                    std::thread::sleep(Duration::from_secs(5));
                    if shared().refresh_now.load(Ordering::Relaxed)
                        || shared().rebuild.load(Ordering::Relaxed)
                        || !enabled()
                    {
                        break;
                    }
                }
            }
        })
        .ok();
}

/// Time of the oldest saved list (they are downloaded together).
fn newest_download() -> Option<SystemTime> {
    let mut oldest: Option<SystemTime> = None;
    for (name, _) in LISTS {
        let t = std::fs::metadata(dir().join(format!("{name}.txt")))
            .and_then(|m| m.modified())
            .ok()?;
        oldest = Some(oldest.map_or(t, |o| o.min(t)));
    }
    oldest
}

/// Downloads every list; a list that fails keeps its previous copy.
fn download() -> Result<(), String> {
    let mut errors = Vec::new();
    for (name, url) in LISTS {
        match crate::update::fetch(url, LIST_LIMIT) {
            Ok(data) if data.len() > 100 => {
                let path = dir().join(format!("{name}.txt"));
                let tmp = path.with_extension("tmp");
                if std::fs::write(&tmp, &data)
                    .and_then(|_| std::fs::rename(&tmp, &path))
                    .is_err()
                {
                    errors.push(format!("{name}: disco"));
                }
            }
            Ok(_) => errors.push(format!("{name}: vazia")),
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    if errors.len() == LISTS.len() {
        return Err(errors.join("; "));
    }
    Ok(())
}

/// Builds a new engine from the saved lists and puts it in use.
fn build(wake: &dyn Fn()) {
    let mut set = FilterSet::new(false);
    let mut popups = PopupRules::default();
    for (name, _) in LISTS {
        if let Ok(text) = std::fs::read_to_string(dir().join(format!("{name}.txt"))) {
            PopupRules::parse(&text, &mut popups);
            let options = ParseOptions {
                permissions: if name.starts_with("ublock-") {
                    TRUSTED
                } else {
                    PermissionMask::default()
                },
                ..Default::default()
            };
            set.add_filter_list(text, options);
        }
    }
    *shared().popups.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(popups);
    // the user's rules
    let mine = RULES.read().unwrap_or_else(|e| e.into_inner()).join("\n");
    set.add_filter_list(
        mine,
        ParseOptions {
            permissions: TRUSTED,
            ..Default::default()
        },
    );
    let mut engine = Engine::new_with_filter_set(set);
    engine.use_resources(resources());
    let rules = engine_rules(&engine);
    *shared().engine.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(engine));
    set_status(|s| s.rules = rules);
    wake();
}

/// uBlock Origin's scriptlets and stand-ins.
fn resources() -> Vec<Resource> {
    serde_json::from_str(UBO_RESOURCES).unwrap_or_default()
}

/// The engine does not report its size: the lists' lines that are rules.
fn engine_rules(_engine: &Engine) -> usize {
    LISTS
        .iter()
        .filter_map(|(name, _)| std::fs::read_to_string(dir().join(format!("{name}.txt"))).ok())
        .map(|t| {
            t.lines()
                .filter(|l| {
                    let l = l.trim();
                    !l.is_empty() && !l.starts_with('!') && !l.starts_with('[')
                })
                .count()
        })
        .sum()
}

/// The kind of a request, in the words of the filter lists.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Document,
    Subdocument,
    Script,
    Stylesheet,
    Image,
    Font,
    Media,
    Xhr,
    WebSocket,
    Ping,
    Other,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Document => "document",
            Kind::Subdocument => "subdocument",
            Kind::Script => "script",
            Kind::Stylesheet => "stylesheet",
            Kind::Image => "image",
            Kind::Font => "font",
            Kind::Media => "media",
            Kind::Xhr => "xmlhttprequest",
            Kind::WebSocket => "websocket",
            Kind::Ping => "ping",
            Kind::Other => "other",
        }
    }
}

/// Host of an address ("https://a.b.com/x" -> "a.b.com").
fn host(url: &str) -> &str {
    crate::config::host_of(url)
}

/// The page `source` is in the list of allowed sites (no blocking there).
fn allowed(source: &str, allow: &[String]) -> bool {
    let h = host(source).to_ascii_lowercase();
    allow.iter().any(|a| {
        let a = a.trim().trim_start_matches("*.").to_ascii_lowercase();
        !a.is_empty() && (h == a || h.ends_with(&format!(".{a}")))
    })
}

/// Whether a request of a page should be blocked.
pub fn should_block(url: &str, source: &str, kind: Kind, allow: &[String]) -> bool {
    verdict(url, source, kind, allow) != Verdict::Allow
}

/// What to do with a request of a page (counts it when blocked).
pub fn verdict(url: &str, source: &str, kind: Kind, allow: &[String]) -> Verdict {
    // the page itself is never blocked here
    if kind == Kind::Document || !url.starts_with("http") {
        return Verdict::Allow;
    }
    if allowed(source, allow) {
        return Verdict::Allow;
    }
    let Some(engine) = engine() else {
        return Verdict::Allow;
    };
    let Ok(request) = Request::new(url, source, kind.name(), "get") else {
        return Verdict::Allow;
    };
    let result = engine.check_network_request(&request);
    if !result.should_block() {
        return Verdict::Allow;
    }
    shared().blocked.fetch_add(1, Ordering::Relaxed);
    match result.redirect {
        Some(data) if data.starts_with("data:") => Verdict::Redirect(data),
        _ => Verdict::Block,
    }
}

/// CSS that hides the ad spots of a page: the page's own rules, and the
/// generic ones matching the given classes and ids. None = nothing to hide.
pub fn cosmetic_css(
    url: &str,
    classes: &[String],
    ids: &[String],
    allow: &[String],
) -> Option<String> {
    if allowed(url, allow) {
        return None;
    }
    let engine = engine()?;
    let res = engine.url_cosmetic_resources(url);
    let mut selectors: Vec<String> = res.hide_selectors.into_iter().collect();
    if !res.generichide {
        selectors.extend(engine.hidden_class_id_selectors(classes, ids, &res.exceptions));
    }
    if selectors.is_empty() {
        return None;
    }
    // in chunks: one bad selector would void the whole rule
    let css: String = selectors
        .chunks(500)
        .map(|c| format!("{}{{display:none!important}}\n", c.join(",")))
        .collect();
    Some(css)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_blocks_from_list_text() {
        let mut set = FilterSet::new(false);
        set.add_filter_list(
            "||doubleclick.net^\n##.ad-banner\n@@||allowed.example^".to_string(),
            ParseOptions::default(),
        );
        let engine = Engine::new_with_filter_set(set);
        let req = Request::new(
            "https://securepubads.g.doubleclick.net/tag/js/gpt.js",
            "https://game.example/play",
            "script",
            "get",
        )
        .unwrap();
        assert!(engine.check_network_request(&req).should_block());
        let ok = Request::new(
            "https://cdn.game.example/a.js",
            "https://game.example/",
            "script",
            "get",
        )
        .unwrap();
        assert!(!engine.check_network_request(&ok).should_block());
        let res = engine.url_cosmetic_resources("https://game.example/");
        let generic =
            engine.hidden_class_id_selectors(["ad-banner"], Vec::<&str>::new(), &res.exceptions);
        assert_eq!(generic, vec![".ad-banner".to_string()]);
    }

    #[test]
    fn scriptlets_and_stand_ins() {
        let mut set = FilterSet::new(false);
        set.add_filter_list(
            "game.example##+js(set-constant, adsBlocked, false)
             ||ads.example/lib.js$script,redirect=noopjs
             game.example##+js(trusted-set-cookie, a, b)"
                .to_string(),
            ParseOptions::default(),
        );
        let mut engine = Engine::new_with_filter_set(set);
        let res = resources();
        assert!(res.len() > 100, "uBlock's resources load");
        engine.use_resources(res);
        let script = engine
            .url_cosmetic_resources("https://game.example/play")
            .injected_script;
        assert!(script.contains("adsBlocked"), "{script}");
        // a trusted scriptlet from a list that is not trusted: left out
        assert!(!script.contains("trusted-set-cookie") && !script.contains("trustedSetCookie"));
        assert!(engine
            .url_cosmetic_resources("https://other.example/")
            .injected_script
            .is_empty());
        let req = Request::new(
            "https://ads.example/lib.js",
            "https://game.example/",
            "script",
            "get",
        )
        .unwrap();
        let r = engine.check_network_request(&req);
        assert!(r.should_block());
        let (mime, body) = data_url(r.redirect.as_deref().unwrap()).unwrap();
        assert_eq!(mime, "application/javascript");
        assert!(String::from_utf8(body).unwrap().contains("function"));
    }

    #[test]
    fn element_rules() {
        assert_eq!(
            element_rule("https://www.Game.example/play?x=1", " div.ad > iframe "),
            Some("www.game.example##div.ad > iframe".to_string())
        );
        assert_eq!(element_rule("https://game.example/", ""), None);
        assert_eq!(
            element_rule("http://localhost:8765/x", "#ad"),
            Some("localhost###ad".to_string())
        );
        assert_eq!(hostname("http://[::1]:80/"), "[::1]");
        assert_eq!(element_rule("about:blank", "div"), None);
    }

    #[test]
    fn popup_rules() {
        let mut r = PopupRules::default();
        PopupRules::parse(
            "||popnet.example^$popup
||scoped.example^$popup,domain=x.com
&popunder=$popup
             /earn.php?z=*&ad=$popup,third-party
@@||good.example^$popup
||net.example^",
            &mut r,
        );
        assert!(r.matches("https://popnet.example/go"));
        assert!(r.matches("https://cdn.popnet.example/go"));
        assert!(!r.matches("https://scoped.example/"));
        assert!(r.matches("https://a.b/x?y=1&popunder=1"));
        assert!(r.matches("https://a.b/earn.php?z=5&ad=1"));
        assert!(!r.matches("https://a.b/earn.php?ad=1&z=5"));
        assert!(!r.matches("https://net.example/"));
        assert!(!r.matches("https://accounts.google.com/o/oauth2"));
    }

    #[test]
    fn hidden_addresses() {
        // a pop-up network's address carrying the ad's one in base64
        let popup = "http://lremsteo.net/_x/aHR0cHM6Ly9wdC5wb3JuaHViLmNvbS9feGEvYWRzP3pvbmVfaWQ9MTk4NDI2MSZyZWRpcmVjdD0xJmZvcm1hdD1wb3B1bmRlcg";
        let urls = embedded_urls(popup);
        assert_eq!(
            urls,
            vec!["https://pt.pornhub.com/_xa/ads?zone_id=1984261&redirect=1&format=popunder"]
        );
        let mut r = PopupRules::default();
        PopupRules::parse("/_xa/ads?$popup", &mut r);
        assert!(!r.matches(popup));
        assert!(r.matches(&urls[0]));
        assert_eq!(
            embedded_urls("https://r.example/go?u=https%3A%2F%2Fads.example%2Fx&b=1"),
            vec!["https://ads.example/x"]
        );
        // ordinary addresses carry nothing
        assert!(embedded_urls("https://accounts.google.com/o/oauth2/v2/auth?client_id=123abcdefghijklmnop&redirect_uri=https%3A%2F%2Fgame.example%2Fcb").len() == 1);
        assert!(embedded_urls("https://game.example/play/level-12345678901234567890").is_empty());
    }

    #[test]
    fn allowed_sites() {
        let allow = vec!["game.example".to_string(), "*.other.io".to_string()];
        assert!(allowed("https://game.example/x", &allow));
        assert!(allowed("https://www.game.example/x", &allow));
        assert!(allowed("https://a.other.io/", &allow));
        assert!(!allowed("https://notgame.example/", &allow));
    }
}
