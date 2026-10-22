//! One open account: its Firefox window placed inside the main window.
//!
//! Isolation: each account is a container of its network's Firefox (see
//! `firefox`): cookies, logins, storage and cache stay apart, while the
//! browser itself is shared - one Firefox for the accounts without a VPN and
//! one per group with a VPN.
//!
//! Opening is asynchronous ([`open`]): Firefox may have to start, then the
//! extension opens the window and Idlerx embeds it. The window keeps
//! responding meanwhile.

use crate::config::{self, Account, Theme};
use crate::firefox::{self, profile, Page, Window};
use eframe::egui;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

pub use crate::firefox::{Failure, Hotkey};

/// Result of [`open`], filled in when the page exists (or failed).
pub type PanelSlot = Rc<RefCell<Option<Result<Panel, String>>>>;

/// Wakes the interface up (a repaint).
pub type Wake = Rc<dyn Fn()>;

/// How an account's browser runs (fixed while that browser runs).
pub struct Browser<'a> {
    /// Hidden accounts keep their game loop running (a few frames a second).
    pub keep_active: bool,
    /// Few page processes for every account.
    pub economy: bool,
    /// Group with a VPN: (group id, port of its gate).
    pub vpn: Option<(&'a str, u16)>,
    /// InPrivate: the page keeps nothing and opens on a blank page.
    pub in_private: bool,
    /// Frames per second of every page (0 = the screen's rate).
    pub fps: u32,
}

/// Network (Firefox) of an account: `main`, or one per group with a VPN.
pub fn network(vpn: Option<&str>) -> String {
    match vpn {
        Some(g) => format!("vpn_{g}"),
        None => "main".into(),
    }
}

/// Starts opening an account inside `parent` (the main window). `wake` is
/// called when the slot is filled.
pub fn open(parent: isize, account: &Account, browser: Browser, wake: Wake) -> PanelSlot {
    let slot: PanelSlot = Rc::new(RefCell::new(None));
    let net = network(browser.vpn.map(|(g, _)| g));
    let spec = firefox::OpenSpec {
        id: account.id.clone(),
        profile: account.profile.clone(),
        url: if browser.in_private {
            "about:blank".into()
        } else {
            account.start_url().to_string()
        },
        private: browser.in_private,
        zoom: account.zoom,
        muted: account.muted,
        net: net.clone(),
        options: profile::Options {
            economy: browser.economy,
            fps: browser.fps,
            proxy: browser.vpn.map(|(_, port)| port),
            debugger: None,
        },
    };
    let s = slot.clone();
    let keep_active = browser.keep_active;
    let opened = firefox::with(|e| {
        e.set_parent(parent);
        e.open(
            spec,
            Box::new(move |r| {
                *s.borrow_mut() = Some(r.map(|page| Panel::new(page, parent, keep_active)));
                wake();
            }),
        );
    });
    if opened.is_none() {
        *slot.borrow_mut() = Some(Err("motor não iniciado".into()));
    }
    slot
}

pub struct Panel {
    page: Rc<Page>,
    window: Window,
    parent: isize,
    pub started: Instant,
    /// Firefox's main process of the account's network.
    pub pid: u32,
    keep_active: bool,
    bounds: Option<(i32, i32, i32, i32)>,
    visible: bool,
    /// Hidden state last told to the page (it runs its loop slower then).
    hidden_sent: Cell<Option<bool>>,
}

impl Drop for Panel {
    fn drop(&mut self) {
        let id = self.page.id.clone();
        firefox::with(|e| e.close(&id));
    }
}

impl Panel {
    fn new(page: Rc<Page>, parent: isize, keep_active: bool) -> Panel {
        let window = page.window.get().expect("embedded window");
        let pid = window.pid();
        let p = Panel {
            page,
            window,
            parent,
            started: Instant::now(),
            pid,
            keep_active,
            bounds: None,
            visible: false,
            hidden_sent: Cell::new(None),
        };
        p.tell_hidden(true);
        p
    }

    fn send(&self, mut msg: serde_json::Value) {
        msg["id"] = self.page.id.clone().into();
        let net = self.page.net.clone();
        firefox::with(|e| e.send(&net, msg));
    }

    fn tell_hidden(&self, hidden: bool) {
        if self.hidden_sent.replace(Some(hidden)) != Some(hidden) {
            self.send(serde_json::json!({
                "cmd": "hidden",
                "on": hidden,
                "awake": self.keep_active,
            }));
        }
    }

    /// Network (Firefox) of the account.
    pub fn network(&self) -> &str {
        &self.page.net
    }

    /// Puts the page on `rect` (logical points of the main window), or hides
    /// it with None. Returns the new visibility when it changed.
    pub fn place(&mut self, rect: Option<egui::Rect>, pixels_per_point: f32) -> Option<bool> {
        match rect {
            Some(r) if r.width() >= 1.0 && r.height() >= 1.0 => {
                let b = (
                    (r.left() * pixels_per_point).round() as i32,
                    (r.top() * pixels_per_point).round() as i32,
                    (r.right() * pixels_per_point).round() as i32,
                    (r.bottom() * pixels_per_point).round() as i32,
                );
                if self.bounds != Some(b) || !self.visible {
                    self.bounds = Some(b);
                    self.window.show_at(b);
                }
                if !self.visible {
                    self.visible = true;
                    self.tell_hidden(false);
                    return Some(true);
                }
                None
            }
            _ => {
                if self.visible {
                    self.visible = false;
                    // hidden: Firefox stops drawing it, and its game loop
                    // runs a few times a second (or not at all, without
                    // "keep games running")
                    self.window.hide();
                    self.tell_hidden(true);
                    return Some(false);
                }
                None
            }
        }
    }

    /// Follows the app's theme: the pages' preferred color scheme.
    pub fn set_theme(&self, theme: Theme, _dark: bool) {
        let scheme = match theme {
            Theme::System => "system",
            Theme::Dark => "dark",
            Theme::Light => "light",
        };
        firefox::with(|e| e.set_scheme(scheme));
    }

    pub fn set_muted(&self, muted: bool) {
        self.send(serde_json::json!({ "cmd": "mute", "muted": muted }));
    }

    /// Some page of the account is playing sound.
    pub fn playing_audio(&self) -> bool {
        self.page.audible.get()
    }

    pub fn set_zoom(&self, zoom: f64) {
        self.page.zoom.set(zoom);
        self.send(serde_json::json!({ "cmd": "zoom", "zoom": zoom.clamp(0.3, 5.0) }));
    }

    // -------------------------------------------------------------- macros --

    /// A click at a point given as a fraction of the page's width and
    /// height (window messages: the Windows mouse is not used). False when
    /// the page size is not known yet.
    pub fn click(&self, fx: f32, fy: f32, button: config::MouseButton, double: bool) -> bool {
        self.page.macro_input.set(Some(Instant::now()));
        self.window
            .click(fx, fy, button == config::MouseButton::Right, double)
    }

    /// Presses and releases a key (see `macros::KEYS`).
    pub fn key(&self, k: &crate::macros::Key) {
        self.page.macro_input.set(Some(Instant::now()));
        self.window.key(k.vk as u16, k.text);
    }

    /// Starts or stops reporting the user's clicks and keys (macro
    /// recording).
    pub fn record(&self, on: bool) {
        self.page.recording.set(on);
        if !on {
            self.page.recorded.borrow_mut().clear();
        }
        self.send(serde_json::json!({ "cmd": "record", "on": on }));
    }

    /// A macro runs on the account (Firefox's own menu stays shut).
    pub fn set_macro(&self, on: bool) {
        self.send(serde_json::json!({ "cmd": "macro", "on": on }));
    }

    /// Events reported since the last call, while recording.
    pub fn take_recorded(&self) -> Vec<serde_json::Value> {
        std::mem::take(&mut self.page.recorded.borrow_mut())
    }

    /// A picture of the page (PNG), delivered to `done` (None if it could
    /// not be taken).
    pub fn capture(&self, done: impl FnOnce(Option<Vec<u8>>) + 'static) {
        let page = self.page.clone();
        if firefox::with(|e| e.capture(&page, done)).is_none() {
            // no engine: nothing to wait for
        }
    }

    /// Current zoom (it also changes with Ctrl+wheel inside the page).
    pub fn zoom(&self) -> f64 {
        self.page.zoom.get()
    }

    pub fn url(&self) -> Option<String> {
        Some(self.page.url.borrow().clone()).filter(|u| !u.is_empty())
    }

    pub fn load(&self, url: &str) {
        self.send(serde_json::json!({ "cmd": "nav", "url": url }));
    }

    pub fn back(&self) {
        self.send(serde_json::json!({ "cmd": "back" }));
    }

    pub fn forward(&self) {
        self.send(serde_json::json!({ "cmd": "forward" }));
    }

    pub fn can_go_back(&self) -> bool {
        true
    }

    pub fn can_go_forward(&self) -> bool {
        true
    }

    pub fn reload(&self) {
        self.send(serde_json::json!({ "cmd": "reload" }));
    }

    /// uBlock Origin's panel for this account's page, at a point of the
    /// screen.
    pub fn open_blocker_panel(&self, x: i32, y: i32) {
        self.send(serde_json::json!({ "cmd": "ublock", "x": x, "y": y }));
    }

    pub fn focus(&self) {
        self.window.focus();
    }

    /// Gives the keyboard back to the main window (egui).
    pub fn focus_parent(&self) {
        #[link(name = "user32")]
        extern "system" {
            fn SetFocus(hwnd: isize) -> isize;
        }
        unsafe { SetFocus(self.parent) };
    }

    /// The page got the keyboard focus since the last call.
    pub fn take_got_focus(&self) -> bool {
        self.page.got_focus.replace(false)
    }

    pub fn take_hotkey(&self) -> Option<Hotkey> {
        self.page.hotkey.take()
    }

    /// A new document started loading since the last call (first load,
    /// reload, navigation).
    pub fn take_loaded(&self) -> bool {
        self.page.loaded.take()
    }

    /// Something failed since the last call.
    pub fn take_failure(&self) -> Option<Failure> {
        self.page.failure.take()
    }
}

/// The browser Idlerx needs: the installed Firefox's version, or why not.
pub fn runtime_version() -> Result<String, String> {
    firefox::runtime_version()
}
