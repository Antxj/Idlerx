//! Idlerx's Firefox profiles, one per network (`main`, and one per group
//! with a VPN), apart from the user's own Firefox: the settings Idlerx
//! needs (and the ones that save resources), the interface hidden in the
//! account windows, uBlock Origin and Idlerx's extension.

use std::path::{Path, PathBuf};

/// Idlerx's extension, signed by Mozilla (empty in development builds: it
/// is then installed for the session through the debugger, see `rdp`).
static XPI: &[u8] = include_bytes!(env!("IDLERX_FIREFOX_XPI"));
pub const EXTENSION_ID: &str = "idlerx@antxj.github.io";
/// Folder of the extension's source (development builds: the executable is
/// in rust/target/<profile>, the source in firefox/extension).
pub fn extension_source() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let repo = exe.parent()?.parent()?.parent()?.parent()?;
    let dir = repo.join("firefox").join("extension");
    dir.join("manifest.json").exists().then_some(dir)
}

const UBLOCK_ID: &str = "uBlock0@raymondhill.net";
const UBLOCK_URL: &str =
    "https://addons.mozilla.org/firefox/downloads/latest/ublock-origin/latest.xpi";
/// Fixed internal addresses of the extensions (Idlerx opens uBlock
/// Origin's panel by it). Same value as in background.js.
const UUIDS: &str = r#"{"uBlock0@raymondhill.net":"9c3c1f0e-1d6b-4f0e-9a8e-1d3e5b7a2c40","idlerx@antxj.github.io":"4f1e2d3c-5b6a-4789-8a0b-1c2d3e4f5a6b"}"#;

/// How a network's Firefox runs (fixed while it runs).
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Few page processes for every account (less memory).
    pub economy: bool,
    /// Frames per second of every page (0 = the screen's own rate).
    pub fps: u32,
    /// Group with a VPN: port of its gate (see `vpn::proxy`).
    pub proxy: Option<u16>,
    /// Development: port of the debugger, to install the extension.
    pub debugger: Option<u16>,
}

pub fn root() -> PathBuf {
    crate::config::profiles_dir().join("firefox")
}

pub fn dir(net: &str) -> PathBuf {
    root().join(net)
}

pub fn signed_extension() -> bool {
    !XPI.is_empty()
}

fn pref(out: &mut String, name: &str, value: impl Into<serde_json::Value>) {
    out.push_str(&format!("user_pref({:?}, {});\n", name, value.into()));
}

/// The profile's settings (user.js, applied by Firefox at every start).
pub fn prefs(o: &Options) -> String {
    let mut s = String::from("// Written by Idlerx at every start: changes here are lost.\n");
    let p = &mut s;
    // a plain start: no welcome, terms, default browser or session pages
    for (k, v) in [
        ("browser.shell.checkDefaultBrowser", false),
        ("browser.aboutwelcome.enabled", false),
        ("browser.preonboarding.enabled", false),
        ("termsofuse.bypassNotification", true),
        (
            "datareporting.policy.dataSubmissionPolicyBypassNotification",
            true,
        ),
        ("trailhead.firstrun.didSeeAboutWelcome", true),
        ("browser.startup.firstrunSkipsHomepage", true),
        ("browser.sessionstore.resume_from_crash", false),
        ("browser.tabs.warnOnClose", false),
        ("browser.warnOnQuit", false),
        ("dom.disable_beforeunload", true),
        ("browser.newtabpage.enabled", false),
    ] {
        pref(p, k, v);
    }
    pref(p, "browser.toolbars.bookmarks.visibility", "never");
    pref(p, "browser.startup.page", 0);
    pref(p, "browser.startup.homepage", "about:blank");
    pref(p, "browser.startup.homepage_override.mstone", "ignore");
    pref(p, "startup.homepage_welcome_url", "");
    pref(p, "startup.homepage_welcome_url.additional", "");
    pref(p, "browser.sessionstore.interval", 60000);
    // no early window while Firefox loads: it would show on the screen
    // before Idlerx can hide it (the start window opens minimized)
    pref(p, "browser.startup.preXulSkeletonUI", false);
    pref(p, "browser.startup.blankWindow", false);
    // never the "troubleshoot mode?" question after Firefox was ended by
    // force (Idlerx closing, a crash): it would hold the start
    pref(p, "toolkit.startup.max_resumed_crashes", -1);
    pref(p, "toolkit.startup.recent_crashes", 0);
    // no telemetry, studies, sponsored content, AI or suggestions
    for k in [
        "datareporting.healthreport.uploadEnabled",
        "datareporting.policy.dataSubmissionEnabled",
        "toolkit.telemetry.enabled",
        "toolkit.telemetry.unified",
        "toolkit.telemetry.archive.enabled",
        "app.normandy.enabled",
        "app.shield.optoutstudies.enabled",
        "extensions.pocket.enabled",
        "browser.discovery.enabled",
        "extensions.getAddons.cache.enabled",
        "browser.ml.enable",
        "browser.ml.chat.enabled",
        "browser.urlbar.speculativeConnect.enabled",
        "browser.newtabpage.activity-stream.feeds.telemetry",
        "browser.newtabpage.activity-stream.telemetry",
        "browser.ping-centre.telemetry",
        "toolkit.cosmeticAnimations.enabled",
        "sidebar.revamp",
        "sidebar.verticalTabs",
    ] {
        pref(p, k, false);
    }
    // Firefox's own connections Idlerx does not need: telemetry pings,
    // sponsored content, the region lookup, push, captive portal checks,
    // suggestions and Mozilla's messages (it still checks for its own
    // updates, blocklists and certificates, and the add-ons' updates)
    for k in [
        "datareporting.usage.uploadEnabled",
        "toolkit.telemetry.newProfilePing.enabled",
        "toolkit.telemetry.shutdownPingSender.enabled",
        "toolkit.telemetry.firstShutdownPing.enabled",
        "toolkit.telemetry.updatePing.enabled",
        "toolkit.telemetry.bhrPing.enabled",
        "browser.region.update.enabled",
        "dom.push.enabled",
        "network.captive-portal-service.enabled",
        "network.connectivity-service.enabled",
        "browser.topsites.contile.enabled",
        "browser.newtabpage.activity-stream.showSponsored",
        "browser.newtabpage.activity-stream.showSponsoredTopSites",
        "browser.newtabpage.activity-stream.feeds.topsites",
        "browser.newtabpage.activity-stream.feeds.section.topstories",
        "browser.newtabpage.activity-stream.feeds.system.topstories",
        "browser.newtabpage.activity-stream.asrouter.userprefs.cfr.addons",
        "browser.newtabpage.activity-stream.asrouter.userprefs.cfr.features",
        "browser.urlbar.quicksuggest.enabled",
        "browser.urlbar.suggest.quicksuggest.sponsored",
        "browser.urlbar.suggest.quicksuggest.nonsponsored",
        "browser.shopping.experience2023.enabled",
        "browser.contentanalysis.enabled",
        "messaging-system.rsexperimentloader.enabled",
        "nimbus.profilesdatastoreservice.enabled",
        "geo.enabled",
    ] {
        pref(p, k, false);
    }
    pref(p, "toolkit.telemetry.server", "data:,");
    pref(p, "browser.region.network.url", "");
    // uBlock Origin already blocks malware and tracking sites: Firefox's own
    // lists (downloaded and kept in memory) are left out
    for k in [
        "browser.safebrowsing.malware.enabled",
        "browser.safebrowsing.phishing.enabled",
        "browser.safebrowsing.downloads.enabled",
        "browser.safebrowsing.blockedURIs.enabled",
    ] {
        pref(p, k, false);
    }
    // no work ahead of time
    pref(p, "network.prefetch-next", false);
    pref(p, "network.dns.disablePrefetch", true);
    pref(p, "network.predictor.enabled", false);
    pref(p, "dom.ipc.processPrelaunch.enabled", false);
    // a smaller disk cache per network
    pref(p, "browser.cache.disk.smart_size.enabled", false);
    pref(p, "browser.cache.disk.capacity", 262_144);
    // passwords and forms: kept by Idlerx per account (see `passwords`)
    for k in [
        "signon.rememberSignons",
        "signon.autofillForms",
        "signon.generation.enabled",
        "extensions.formautofill.addresses.enabled",
        "extensions.formautofill.creditCards.enabled",
    ] {
        pref(p, k, false);
    }
    // containers (one per account) and Idlerx's own interface
    pref(p, "privacy.userContext.enabled", true);
    pref(
        p,
        "toolkit.legacyUserProfileCustomizations.stylesheets",
        true,
    );
    pref(p, "browser.tabs.inTitlebar", 0);
    pref(p, "browser.zoom.siteSpecific", false);
    // windows the pages open become windows of their own (with an address
    // bar), never hidden tabs of the account's window
    pref(p, "browser.link.open_newwindow", 2);
    pref(p, "browser.link.open_newwindow.restriction", 0);
    // games may play sound without a click first
    pref(p, "media.autoplay.default", 0);
    // the games keep running at full speed when not on screen
    pref(p, "dom.min_background_timeout_value", 4);
    pref(p, "dom.timeout.enable_budget_timer_throttling", false);
    pref(p, "widget.windows.window_occlusion_tracking.enabled", false);
    pref(p, "dom.suspend_inactive.enabled", false);
    pref(p, "browser.tabs.unloadOnLowMemory", false);
    pref(
        p,
        "dom.ipc.processPriorityManager.backgroundUsesEcoQoS",
        false,
    );
    // the frame limit, done by Firefox itself
    pref(
        p,
        "layout.frame_rate",
        if o.fps > 0 { o.fps as i64 } else { -1 },
    );
    // nothing kept that Idlerx does not use: history (back and forward
    // still work), closed windows, translations, accessibility services
    pref(p, "places.history.enabled", false);
    pref(p, "browser.sessionstore.max_tabs_undo", 0);
    pref(p, "browser.sessionstore.max_windows_undo", 0);
    pref(p, "browser.translations.enable", false);
    pref(p, "accessibility.force_disabled", 1);
    // the network in the main process (one process less)
    pref(p, "network.process.enabled", false);
    if o.economy {
        // fewer processes: media and the extensions (uBlock Origin,
        // Idlerx's) inside the main one. Each account keeps its own page
        // process (Firefox gives every container its own). Drawing stays
        // in its own process: inside the main one, embedded windows stop
        // painting.
        pref(p, "fission.webContentIsolationStrategy", 0);
        pref(p, "media.rdd-process.enabled", false);
        pref(p, "extensions.webextensions.remote", false);
    }
    // extensions in the profile folder start enabled; fixed internal
    // addresses
    pref(p, "extensions.autoDisableScopes", 0);
    // a new version of an extension in the profile folder (Idlerx's, after
    // an update of Idlerx) is installed when Firefox starts, not while it
    // runs: that restarted the extension and dropped its link to Idlerx
    pref(p, "extensions.startupScanScopes", 1);
    pref(p, "extensions.webextensions.uuids", UUIDS);
    if let Some(port) = o.proxy {
        // group with a VPN: everything through its gate, nothing direct
        pref(p, "network.proxy.type", 1);
        pref(p, "network.proxy.socks", "127.0.0.1");
        pref(p, "network.proxy.socks_port", port);
        pref(p, "network.proxy.socks_version", 5);
        pref(p, "network.proxy.socks_remote_dns", true);
        pref(p, "network.proxy.no_proxies_on", "");
        pref(p, "network.proxy.allow_hijacking_localhost", true);
        pref(p, "network.proxy.failover_direct", false);
        pref(p, "network.trr.mode", 5);
        pref(p, "network.http.http3.enable", false);
        pref(p, "network.dns.disableIPv6", true);
        // WebRTC only through the proxy, which carries no UDP
        pref(p, "media.peerconnection.ice.proxy_only", true);
        pref(p, "media.peerconnection.ice.default_address_only", true);
        pref(p, "media.peerconnection.ice.no_host", true);
    } else {
        pref(p, "network.proxy.type", 5);
    }
    if o.debugger.is_some() {
        pref(p, "devtools.debugger.remote-enabled", true);
        pref(p, "devtools.chrome.enabled", true);
        pref(p, "devtools.debugger.prompt-connection", false);
    }
    s
}

/// Firefox's interface hidden in the account windows (their title has the
/// marker): only the page. Windows the pages open keep it.
const USER_CHROME: &str = r#"/* Written by Idlerx. */
:root[titlepreface*="idlerx:"] #navigator-toolbox,
:root[titlepreface*="idlerx:"] #sidebar-box,
:root[titlepreface*="idlerx:"] #sidebar-main,
:root[titlepreface*="idlerx:"] #sidebar-splitter {
  visibility: collapse !important;
}
"#;

fn write_if_changed(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if std::fs::read(path).ok().as_deref() != Some(bytes) {
        std::fs::write(path, bytes)?;
    }
    Ok(())
}

/// Creates or updates a network's profile; returns its folder.
pub fn prepare(net: &str, o: &Options) -> Result<PathBuf, String> {
    let dir = dir(net);
    let chrome = dir.join("chrome");
    let ext = dir.join("extensions");
    for d in [&chrome, &ext] {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    write_if_changed(&dir.join("user.js"), prefs(o).as_bytes()).map_err(|e| e.to_string())?;
    write_if_changed(&chrome.join("userChrome.css"), USER_CHROME.as_bytes())
        .map_err(|e| e.to_string())?;
    if signed_extension() {
        write_if_changed(&ext.join(format!("{EXTENSION_ID}.xpi")), XPI)
            .map_err(|e| e.to_string())?;
    }
    // uBlock Origin: installed once, then kept up to date by Firefox
    let ublock = ext.join(format!("{UBLOCK_ID}.xpi"));
    if !ublock.exists() {
        if let Some(cached) = ublock_xpi() {
            let _ = std::fs::copy(cached, &ublock);
        }
    }
    Ok(dir)
}

/// uBlock Origin from Mozilla's add-ons site, kept for new profiles (Firefox
/// checks its signature when installing). None when it cannot be had.
fn ublock_xpi() -> Option<PathBuf> {
    let path = root().join("ublock-origin.xpi");
    if path.exists() {
        return Some(path);
    }
    let bytes = crate::update::fetch(UBLOCK_URL, 20 << 20).ok()?;
    // an xpi is a zip
    if !bytes.starts_with(b"PK") {
        return None;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &bytes).ok()?;
    std::fs::rename(&tmp, &path).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vpn_prefs_send_everything_to_the_gate() {
        let o = Options {
            economy: true,
            fps: 30,
            proxy: Some(4321),
            debugger: None,
        };
        let s = prefs(&o);
        assert!(s.contains(r#"user_pref("network.proxy.socks_port", 4321);"#));
        assert!(s.contains(r#"user_pref("network.proxy.failover_direct", false);"#));
        assert!(s.contains(r#"user_pref("network.proxy.socks_remote_dns", true);"#));
        assert!(s.contains(r#"user_pref("layout.frame_rate", 30);"#));
        assert!(!s.contains("devtools.debugger.remote-enabled"));
        let plain = prefs(&Options {
            proxy: None,
            fps: 0,
            ..o
        });
        assert!(plain.contains(r#"user_pref("network.proxy.type", 5);"#));
        assert!(plain.contains(r#"user_pref("layout.frame_rate", -1);"#));
    }
}

#[cfg(test)]
mod print {
    /// Writes a VPN network's user.js (for the proxy leak test).
    #[test]
    #[ignore]
    fn write_vpn_prefs() {
        let o = super::Options {
            economy: true,
            fps: 30,
            proxy: Some(18765),
            debugger: None,
        };
        std::fs::write(std::env::var("OUT_USER_JS").unwrap(), super::prefs(&o)).unwrap();
    }
}
