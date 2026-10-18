//! One open account: a WebView2 page placed inside the main window.
//!
//! Isolation: each account uses its own profile folder, so WebView2 starts a
//! separate browser process for it, with its own cookies, cache, storage and
//! logins. If one account crashes, the others keep running.
//!
//! Economy mode: one browser (a shared folder) with one WebView2 profile per
//! account. Cookies, cache and logins stay separate, but the browser, GPU and
//! network processes are shared - less memory, and a crash there takes every
//! account down. Each account still has its own page (renderer) processes.
//!
//! Opening is fully asynchronous ([`open`]): the browser environment and then
//! the page are created through WebView2 callbacks, delivered by the normal
//! window message loop. Nothing waits in a nested message loop - creating
//! several pages back to back that way (as wry does) can hang the app.

use crate::config::{self, Account, Theme};
use eframe::egui;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler,
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, ContentLoadingEventHandler,
    CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, ExecuteScriptCompletedHandler,
    FocusChangedEventHandler, GetProcessExtendedInfosCompletedHandler, ProcessFailedEventHandler,
};
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, CapturePreviewCompletedHandler,
    DOMContentLoadedEventHandler, WebMessageReceivedEventHandler, WebResourceRequestedEventHandler,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows_core::{Interface, BOOL, HSTRING, PCWSTR, PWSTR};

/// WebView2 browser arguments. With `keep_active`, hidden or covered pages
/// keep their timers at full speed (idle games keep playing while another
/// account is on screen).
fn browser_args(keep_active: bool, proxy: Option<u16>) -> String {
    // mini menu, PDF menu and SmartScreen off (same as wry's defaults)
    let mut s = String::from("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
    if keep_active {
        s.push_str(
            ",CalculateNativeWinOcclusion,IntensiveWakeUpThrottling \
             --disable-background-timer-throttling \
             --disable-renderer-backgrounding \
             --disable-backgrounding-occluded-windows",
        );
    }
    // games may play sound without a click first
    s.push_str(" --autoplay-policy=no-user-gesture-required");
    if let Some(port) = proxy {
        s.push(' ');
        s.push_str(&vpn_args(port));
    }
    s
}

/// Group with a VPN: every request goes through the group's gate (see
/// `vpn::proxy`), with nothing that could reach the regular network.
fn vpn_args(port: u16) -> String {
    [
        // only the gate, even for local addresses (the gate refuses them)
        format!("--proxy-server=socks5://127.0.0.1:{port}"),
        "--proxy-bypass-list=<-loopback>".into(),
        // names are resolved by the gate, through the tunnel: the browser
        // itself cannot resolve anything
        "\"--host-resolver-rules=MAP * ~NOTFOUND , EXCLUDE 127.0.0.1\"".into(),
        // WebRTC only through the proxy, which carries no UDP: no STUN
        // request reveals the real address
        "--force-webrtc-ip-handling-policy=disable_non_proxied_udp".into(),
        "--disable-quic".into(),
    ]
    .join(" ")
}

/// Limits how often the pages draw (see the file).
const FPS_SCRIPT: &str = include_str!("fps.js");

/// Keys pressed inside a page that belong to the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hotkey {
    /// F11: focus mode.
    Focus,
    /// Esc: leaves focus mode (the page still gets it).
    Escape,
}

/// A process of the page failed (see [`Panel::take_failure`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The page's process (or one of its frames') ended: the page is blank.
    PageCrashed,
    /// The page stopped responding.
    Unresponsive,
    /// The browser process ended: the page is gone for good.
    BrowserGone,
}

/// Result of [`open`], filled in when the page exists (or failed).
pub type PanelSlot = Rc<RefCell<Option<Result<Panel, String>>>>;

/// Page processes of each page: (browser pid, main frame id) -> pids.
pub type Renderers = HashMap<(u32, u32), Vec<u32>>;

/// Wakes the interface up (a repaint) from a WebView2 callback.
pub type Wake = Rc<dyn Fn()>;

/// How an account's browser is started (fixed while that browser runs).
pub struct Browser<'a> {
    /// No background throttling (see `browser_args`).
    pub keep_active: bool,
    /// Shared browser, one profile per account.
    pub economy: bool,
    /// Group with a VPN: (group id, port of its gate).
    pub vpn: Option<(&'a str, u16)>,
    /// InPrivate: the page keeps nothing and opens on a blank page.
    pub in_private: bool,
}

/// Starts opening an account inside `parent` (the main window). `wake` is
/// called on the UI thread when the slot is filled.
pub fn open(
    parent: isize,
    account: &Account,
    browser: Browser,
    save_passwords: bool,
    dark: bool,
    wake: Wake,
) -> PanelSlot {
    let Browser {
        keep_active,
        economy,
        vpn,
        in_private,
    } = browser;
    let slot: PanelSlot = Rc::new(RefCell::new(None));
    // a browser's arguments (the proxy) are fixed for its whole folder: in
    // economy mode each group with a VPN has a shared browser of its own
    let dir = match (economy, vpn) {
        (true, Some((group, _))) => config::vpn_shared_dir(group),
        (true, None) => config::shared_dir(),
        (false, _) => config::profile_dir(&account.profile),
    };
    let _ = std::fs::create_dir_all(&dir);
    let options = CoreWebView2EnvironmentOptions::default();
    let args = browser_args(keep_active, vpn.map(|(_, port)| port));
    unsafe { options.set_additional_browser_arguments(args) };

    let s = slot.clone();
    let mut account = account.clone();
    // InPrivate: always a fresh start, on a blank page, with nothing saved
    let save_passwords = save_passwords && !in_private;
    if in_private {
        account.url = "about:blank".into();
        account.last_url = None;
    }
    let env_done =
        CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |res, env| {
            let env = match res
                .map_err(|e| e.to_string())
                .and_then(|_| env.ok_or_else(|| "WebView2 returned no environment".to_string()))
            {
                Ok(env) => env,
                Err(e) => {
                    *s.borrow_mut() = Some(Err(e));
                    wake();
                    return Ok(());
                }
            };
            let s2 = s.clone();
            let env2 = env.clone();
            let profile = account.profile.clone();
            let controller_done = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                move |res, controller| {
                    let r = res
                        .map_err(|e| e.to_string())
                        .and_then(|_| {
                            controller.ok_or_else(|| "WebView2 returned no page".to_string())
                        })
                        .and_then(|c| {
                            Panel::setup(
                                c,
                                &env2,
                                parent,
                                &account,
                                save_passwords,
                                dark,
                                wake.clone(),
                            )
                            .map_err(|e| e.to_string())
                        });
                    *s2.borrow_mut() = Some(r);
                    wake();
                    Ok(())
                },
            ));
            let created = unsafe {
                if economy || in_private {
                    // economy: the account is a named profile inside the
                    // shared browser; InPrivate: its data stays in memory
                    env.cast::<ICoreWebView2Environment10>().and_then(|env10| {
                        let opts = env10.CreateCoreWebView2ControllerOptions()?;
                        opts.SetProfileName(&HSTRING::from(profile.as_str()))?;
                        if in_private {
                            opts.SetIsInPrivateModeEnabled(true)?;
                        }
                        env10.CreateCoreWebView2ControllerWithOptions(
                            HWND(parent as _),
                            &opts,
                            &controller_done,
                        )
                    })
                } else {
                    env.CreateCoreWebView2Controller(HWND(parent as _), &controller_done)
                }
            };
            if let Err(e) = created {
                *s.borrow_mut() = Some(Err(e.to_string()));
            }
            Ok(())
        }));

    let started = unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &HSTRING::from(dir.as_os_str()),
            &ICoreWebView2EnvironmentOptions::from(options),
            &env_done,
        )
    };
    if let Err(e) = started {
        *slot.borrow_mut() = Some(Err(e.to_string()));
    }
    slot
}

pub struct Panel {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
    env: ICoreWebView2Environment,
    parent: isize,
    pub started: Instant,
    /// Browser process (root of the account's process tree).
    pub pid: u32,
    got_focus: Rc<Cell<bool>>,
    hotkey: Rc<Cell<Option<Hotkey>>>,
    bounds: Option<(i32, i32, i32, i32)>,
    visible: bool,
    /// Frame limit last sent to the page, and "send again" (new document).
    fps_sent: Cell<Option<u32>>,
    fps_resend: Rc<Cell<bool>>,
    /// Frames per second the page actually drew (measured inside it);
    /// None until known or when the page does not animate.
    fps_rate: Rc<Cell<Option<u32>>>,
    /// "Keep awake" last sent with the frame limit.
    awake_sent: Cell<Option<bool>>,
    failure: Rc<Cell<Option<Failure>>>,
    /// A new document started loading since the last call.
    loaded: Rc<Cell<bool>>,
    /// Recording a macro: what the page reported so far.
    recording: Rc<Cell<bool>>,
    recorded: Rc<RefCell<Vec<serde_json::Value>>>,
}

impl Drop for Panel {
    fn drop(&mut self) {
        // closes the page; the account's browser process ends with it
        let _ = unsafe { self.controller.Close() };
    }
}

impl Panel {
    /// Settings and handlers of a freshly created page, then the first
    /// navigation.
    fn setup(
        controller: ICoreWebView2Controller,
        env: &ICoreWebView2Environment,
        parent: isize,
        account: &Account,
        save_passwords: bool,
        dark: bool,
        wake: Wake,
    ) -> windows_core::Result<Panel> {
        unsafe {
            controller.SetIsVisible(false)?;
            if let Ok(c2) = controller.cast::<ICoreWebView2Controller2>() {
                let _ = c2.SetDefaultBackgroundColor(background(dark));
            }
            let webview = controller.CoreWebView2()?;

            let settings = webview.Settings()?;
            let _ = settings.SetIsStatusBarEnabled(false);
            let _ = settings.SetAreDevToolsEnabled(cfg!(debug_assertions));

            let mut pid = 0u32;
            let _ = webview.BrowserProcessId(&mut pid);

            let got_focus = Rc::new(Cell::new(false));
            let hotkey = Rc::new(Cell::new(None));
            let mut token = Default::default();
            let f = got_focus.clone();
            let w = wake.clone();
            let focus_handler = FocusChangedEventHandler::create(Box::new(move |_, _| {
                f.set(true);
                w();
                Ok(())
            }));
            let k = hotkey.clone();
            let wake_keys = wake.clone();
            let key_handler =
                AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = Default::default();
                    let mut vk = 0u32;
                    args.KeyEventKind(&mut kind)?;
                    args.VirtualKey(&mut vk)?;
                    let down = kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN;
                    const VK_F11: u32 = 0x7A;
                    const VK_ESCAPE: u32 = 0x1B;
                    if down && vk == VK_F11 {
                        k.set(Some(Hotkey::Focus));
                        args.SetHandled(true)?;
                        wake_keys();
                    } else if down && vk == VK_ESCAPE {
                        k.set(Some(Hotkey::Escape));
                        wake_keys();
                    }
                    Ok(())
                }));
            // frame limiter: in every document, before the page's scripts
            let _ = webview.AddScriptToExecuteOnDocumentCreated(
                &HSTRING::from(FPS_SCRIPT),
                &AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(|_, _| {
                    Ok(())
                })),
            );
            let fps_resend = Rc::new(Cell::new(true));
            let loaded = Rc::new(Cell::new(false));
            let (r, l) = (fps_resend.clone(), loaded.clone());
            let w = wake.clone();
            let loading = ContentLoadingEventHandler::create(Box::new(move |_, _| {
                r.set(true);
                l.set(true);
                w();
                Ok(())
            }));
            let _ = webview.add_ContentLoading(&loading, &mut token);
            // a crashed or stuck page is reported to the app, which reloads
            // it (or opens the account again when its browser is gone)
            let failure = Rc::new(Cell::new(None));
            let fl = failure.clone();
            let w = wake.clone();
            let failed = ProcessFailedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = Default::default();
                args.ProcessFailedKind(&mut kind)?;
                let f = match kind {
                    COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => {
                        Some(Failure::BrowserGone)
                    }
                    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                    | COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED => {
                        Some(Failure::PageCrashed)
                    }
                    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => {
                        Some(Failure::Unresponsive)
                    }
                    // GPU and helper processes come back by themselves
                    _ => None,
                };
                if let Some(f) = f {
                    // the most serious one wins until the app reads it
                    if fl.get() != Some(Failure::BrowserGone) {
                        fl.set(Some(f));
                    }
                    w();
                }
                Ok(())
            }));
            let _ = webview.add_ProcessFailed(&failed, &mut token);
            // macro recording: the page reports the user's clicks and keys
            let recording = Rc::new(Cell::new(false));
            let recorded = Rc::new(RefCell::new(Vec::new()));
            let (on, list) = (recording.clone(), recorded.clone());
            let w = wake.clone();
            let messages = WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                if !on.get() {
                    return Ok(());
                }
                let mut json = PWSTR::null();
                args.WebMessageAsJson(&mut json)?;
                let text = take_pwstr(json);
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    if v["__idlerxRec"].is_object() {
                        list.borrow_mut().push(v["__idlerxRec"].clone());
                        w();
                    }
                }
                Ok(())
            }));
            let _ = webview.add_WebMessageReceived(&messages, &mut token);
            // ad blocker (see `blocker`): every request of the page, iframes
            // included, is checked; the blocked ones get an empty answer
            if crate::blocker::enabled() {
                watch_requests(&webview, env);
            }
            let _ = controller.add_GotFocus(&focus_handler, &mut token);
            let _ = controller.add_AcceleratorKeyPressed(&key_handler, &mut token);

            let p = Panel {
                controller,
                webview,
                env: env.clone(),
                parent,
                started: Instant::now(),
                pid,
                got_focus,
                hotkey,
                bounds: None,
                visible: false,
                fps_sent: Cell::new(None),
                fps_resend,
                fps_rate: Rc::new(Cell::new(None)),
                awake_sent: Cell::new(None),
                failure,
                loaded,
                recording,
                recorded,
            };
            p.set_save_passwords(save_passwords);
            p.set_muted(account.muted);
            if (account.zoom - 1.0).abs() > 0.001 {
                p.set_zoom(account.zoom);
            }
            p.load(account.start_url());
            Ok(p)
        }
    }

    /// Puts the page on `rect` (logical points of the main window), or hides
    /// it with None. Only calls WebView2 when something changed.
    /// Returns the new visibility when it changed.
    pub fn place(&mut self, rect: Option<egui::Rect>, pixels_per_point: f32) -> Option<bool> {
        match rect {
            Some(r) if r.width() >= 1.0 && r.height() >= 1.0 => {
                let b = (
                    (r.left() * pixels_per_point).round() as i32,
                    (r.top() * pixels_per_point).round() as i32,
                    (r.right() * pixels_per_point).round() as i32,
                    (r.bottom() * pixels_per_point).round() as i32,
                );
                if self.bounds != Some(b) {
                    self.bounds = Some(b);
                    let rect = RECT {
                        left: b.0,
                        top: b.1,
                        right: b.2,
                        bottom: b.3,
                    };
                    let _ = unsafe { self.controller.SetBounds(rect) };
                }
                if !self.visible {
                    self.visible = true;
                    let _ = unsafe { self.controller.SetIsVisible(true) };
                    self.memory_level(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL);
                    return Some(true);
                }
                None
            }
            _ => {
                if self.visible {
                    self.visible = false;
                    // hidden: stops drawing and asks WebView2 to keep less
                    // memory (the page keeps running)
                    let _ = unsafe { self.controller.SetIsVisible(false) };
                    self.memory_level(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
                    return Some(false);
                }
                None
            }
        }
    }

    fn memory_level(&self, level: COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL) {
        if let Ok(w) = self.webview.cast::<ICoreWebView2_19>() {
            let _ = unsafe { w.SetMemoryUsageTargetLevel(level) };
        }
    }

    /// Frames per second the page may draw (0 = no limit). Only talks to
    /// the page when the value changes or a new document loaded.
    /// Frame limit, and `awake`: the game keeps running while the page is
    /// hidden (see fps.js).
    pub fn set_fps(&self, fps: u32, awake: bool) {
        let same = self.fps_sent.get() == Some(fps) && self.awake_sent.get() == Some(awake);
        if !self.fps_resend.replace(false) && same {
            return;
        }
        self.fps_sent.set(Some(fps));
        self.awake_sent.set(Some(awake));
        let js = format!("window.__idlerxFps && window.__idlerxFps.set({fps}, {awake})");
        let _ = unsafe {
            self.webview.ExecuteScript(
                &HSTRING::from(js),
                &ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(()))),
            )
        };
    }

    /// Asks the page how many frames it drew in the last second (the answer
    /// arrives asynchronously, read with [`Panel::fps_rate`]).
    pub fn request_fps_rate(&self) {
        let out = self.fps_rate.clone();
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |res, json| {
            let rate = res
                .ok()
                .and_then(|_| json.trim().parse::<f64>().ok())
                .filter(|r| *r >= 0.0)
                .map(|r| r.round() as u32);
            out.set(rate);
            Ok(())
        }));
        let _ = unsafe {
            self.webview.ExecuteScript(
                &HSTRING::from("window.__idlerxFps ? window.__idlerxFps.rate : -1"),
                &handler,
            )
        };
    }

    /// Frames per second the page actually drew (last measurement).
    pub fn fps_rate(&self) -> Option<u32> {
        self.fps_rate.get()
    }

    /// Edge's password manager: offers to save the login and fills it in
    /// next time (also user names and other form fields). Stored in the
    /// account's profile, encrypted by Windows; no sync.
    pub fn set_save_passwords(&self, on: bool) {
        let Ok(settings) = (unsafe { self.webview.Settings() }) else {
            return;
        };
        if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
            unsafe {
                let _ = s4.SetIsPasswordAutosaveEnabled(on);
                let _ = s4.SetIsGeneralAutofillEnabled(on);
            }
        }
    }

    /// Follows the app's theme instead of Windows': the pages' preferred
    /// color scheme (sites with a dark mode use it) and the background shown
    /// while a page loads.
    pub fn set_theme(&self, theme: Theme, dark: bool) {
        let scheme = match theme {
            Theme::System => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
            Theme::Dark => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
            Theme::Light => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
        };
        unsafe {
            if let Ok(profile) = self
                .webview
                .cast::<ICoreWebView2_13>()
                .and_then(|w| w.Profile())
            {
                let _ = profile.SetPreferredColorScheme(scheme);
            }
            if let Ok(c2) = self.controller.cast::<ICoreWebView2Controller2>() {
                let _ = c2.SetDefaultBackgroundColor(background(dark));
            }
        }
    }

    pub fn set_muted(&self, muted: bool) {
        if let Ok(w) = self.webview.cast::<ICoreWebView2_8>() {
            let _ = unsafe { w.SetIsMuted(muted) };
        }
    }

    /// Some page of the account is playing sound.
    pub fn playing_audio(&self) -> bool {
        let Ok(w) = self.webview.cast::<ICoreWebView2_8>() else {
            return false;
        };
        let mut b = BOOL(0);
        unsafe { w.IsDocumentPlayingAudio(&mut b) }.is_ok() && b.as_bool()
    }

    pub fn set_zoom(&self, zoom: f64) {
        let _ = unsafe { self.controller.SetZoomFactor(zoom.clamp(0.25, 5.0)) };
    }

    // -------------------------------------------------------------- macros --

    /// Size of the page in CSS pixels (what the page's own coordinates use),
    /// from where it was last placed. None if it was never on screen.
    fn css_size(&self) -> Option<(f64, f64)> {
        let b = self.bounds?;
        let mut scale = 1.0;
        if let Ok(c3) = self.controller.cast::<ICoreWebView2Controller3>() {
            let _ = unsafe { c3.RasterizationScale(&mut scale) };
        }
        let factor = (scale * self.zoom()).max(0.01);
        Some(((b.2 - b.0) as f64 / factor, (b.3 - b.1) as f64 / factor))
    }

    /// Sends a DevTools protocol command (input reaches the page directly:
    /// the Windows mouse and keyboard are not involved).
    fn devtools(&self, method: &str, params: String) {
        let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
        let _ = unsafe {
            self.webview.CallDevToolsProtocolMethod(
                &HSTRING::from(method),
                &HSTRING::from(params),
                &handler,
            )
        };
    }

    /// A click at a point given as a fraction of the page's width and
    /// height. False when the page size is not known yet.
    pub fn click(&self, fx: f32, fy: f32, button: config::MouseButton, double: bool) -> bool {
        let Some((w, h)) = self.css_size() else {
            return false;
        };
        let x = (fx.clamp(0.0, 1.0) as f64 * w).round();
        let y = (fy.clamp(0.0, 1.0) as f64 * h).round();
        let (name, mask) = match button {
            config::MouseButton::Left => ("left", 1),
            config::MouseButton::Right => ("right", 2),
        };
        let event = |kind: &str, buttons: u32, count: u32| {
            format!(
                r#"{{"type":"{kind}","x":{x},"y":{y},"button":"{name}","buttons":{buttons},"clickCount":{count},"pointerType":"mouse"}}"#
            )
        };
        let moved = format!(r#"{{"type":"mouseMoved","x":{x},"y":{y}}}"#);
        self.devtools("Input.dispatchMouseEvent", moved);
        let clicks = if double { 2 } else { 1 };
        for count in 1..=clicks {
            self.devtools(
                "Input.dispatchMouseEvent",
                event("mousePressed", mask, count),
            );
            self.devtools("Input.dispatchMouseEvent", event("mouseReleased", 0, count));
        }
        true
    }

    /// Presses and releases a key (see `macros::KEYS`).
    pub fn key(&self, k: &crate::macros::Key) {
        let mut down = serde_json::json!({
            "type": if k.text.is_some() { "keyDown" } else { "rawKeyDown" },
            "key": k.key,
            "code": k.code,
            "windowsVirtualKeyCode": k.vk,
            "nativeVirtualKeyCode": k.vk,
        });
        if let Some(t) = k.text {
            down["text"] = t.into();
        }
        let mut up = down.clone();
        up["type"] = "keyUp".into();
        if let Some(o) = up.as_object_mut() {
            o.remove("text");
        }
        self.devtools("Input.dispatchKeyEvent", down.to_string());
        self.devtools("Input.dispatchKeyEvent", up.to_string());
    }

    /// Starts or stops reporting the user's clicks and keys (macro
    /// recording). Asked again now and then: a new document starts off.
    pub fn record(&self, on: bool) {
        self.recording.set(on);
        let js = format!("window.__idlerxFps && window.__idlerxFps.record({on})");
        let _ = unsafe {
            self.webview.ExecuteScript(
                &HSTRING::from(js),
                &ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(()))),
            )
        };
    }

    /// Events reported since the last call, while recording.
    pub fn take_recorded(&self) -> Vec<serde_json::Value> {
        std::mem::take(&mut self.recorded.borrow_mut())
    }

    /// A picture of the page (PNG), delivered to `done` on the UI thread
    /// (None if it could not be taken).
    pub fn capture(&self, done: impl FnOnce(Option<Vec<u8>>) + 'static) {
        use windows::Win32::System::Com::{IStream, STREAM_SEEK_SET};
        use windows::Win32::UI::Shell::SHCreateMemStream;
        let Some(stream) = (unsafe { SHCreateMemStream(None) }) else {
            done(None);
            return;
        };
        let s2: IStream = stream.clone();
        let done = RefCell::new(Some(done));
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |res| {
            let Some(done) = done.borrow_mut().take() else {
                return Ok(());
            };
            if res.is_err() {
                done(None);
                return Ok(());
            }
            let mut out = Vec::new();
            unsafe {
                let _ = s2.Seek(0, STREAM_SEEK_SET, None);
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let mut read = 0u32;
                    let hr = s2.Read(buf.as_mut_ptr() as _, buf.len() as u32, Some(&mut read));
                    if hr.is_err() || read == 0 {
                        break;
                    }
                    out.extend_from_slice(&buf[..read as usize]);
                }
            }
            done((!out.is_empty()).then_some(out));
            Ok(())
        }));
        let r = unsafe {
            self.webview.CapturePreview(
                COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                &stream,
                &handler,
            )
        };
        if r.is_err() {
            // the handler never runs: nothing to wait for
        }
    }

    /// Current zoom (it also changes with Ctrl+wheel inside the page).
    pub fn zoom(&self) -> f64 {
        let mut z = 1.0;
        let _ = unsafe { self.controller.ZoomFactor(&mut z) };
        z
    }

    pub fn url(&self) -> Option<String> {
        let mut p = PWSTR::null();
        unsafe { self.webview.Source(&mut p) }.ok()?;
        Some(take_pwstr(p)).filter(|u| !u.is_empty())
    }

    pub fn load(&self, url: &str) {
        let _ = unsafe { self.webview.Navigate(&HSTRING::from(url)) };
    }

    pub fn back(&self) {
        let _ = unsafe { self.webview.GoBack() };
    }

    pub fn forward(&self) {
        let _ = unsafe { self.webview.GoForward() };
    }

    pub fn can_go_back(&self) -> bool {
        let mut b = BOOL(0);
        unsafe { self.webview.CanGoBack(&mut b) }.is_ok() && b.as_bool()
    }

    pub fn can_go_forward(&self) -> bool {
        let mut b = BOOL(0);
        unsafe { self.webview.CanGoForward(&mut b) }.is_ok() && b.as_bool()
    }

    pub fn reload(&self) {
        let _ = unsafe { self.webview.Reload() };
    }

    pub fn focus(&self) {
        let _ = unsafe {
            self.controller
                .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)
        };
    }

    /// Gives the keyboard back to the main window (egui).
    pub fn focus_parent(&self) {
        #[link(name = "user32")]
        extern "system" {
            fn SetFocus(hwnd: isize) -> isize;
        }
        unsafe { SetFocus(self.parent) };
    }

    /// Id of the page's main frame (to find its processes in economy mode).
    pub fn main_frame_id(&self) -> Option<u32> {
        let w = self.webview.cast::<ICoreWebView2_20>().ok()?;
        let mut id = 0u32;
        unsafe { w.FrameId(&mut id) }.ok()?;
        Some(id)
    }

    /// Asks the browser which page processes serve each main frame: fills
    /// `out` with main frame id -> process ids (asynchronously).
    pub fn request_renderers(&self, out: Rc<RefCell<Renderers>>, ready: impl Fn() + 'static) {
        // frame ids are only unique inside one browser: keyed by its pid
        let browser = self.pid;
        let Ok(env13) = self.env.cast::<ICoreWebView2Environment13>() else {
            return;
        };
        let handler =
            GetProcessExtendedInfosCompletedHandler::create(Box::new(move |res, list| {
                if res.is_err() {
                    return Ok(());
                }
                let Some(list) = list else { return Ok(()) };
                let mut map: Renderers = HashMap::new();
                unsafe {
                    let mut n = 0u32;
                    list.Count(&mut n)?;
                    for i in 0..n {
                        let Ok(info) = list.GetValueAtIndex(i) else {
                            continue;
                        };
                        let mut pid = 0i32;
                        let mut kind = Default::default();
                        if let Ok(p) = info.ProcessInfo() {
                            let _ = p.ProcessId(&mut pid);
                            let _ = p.Kind(&mut kind);
                        }
                        if kind != COREWEBVIEW2_PROCESS_KIND_RENDERER || pid <= 0 {
                            continue;
                        }
                        let Ok(frames) = info.AssociatedFrameInfos() else {
                            continue;
                        };
                        let Ok(it) = frames.GetIterator() else {
                            continue;
                        };
                        let mut has = BOOL(0);
                        while it.HasCurrent(&mut has).is_ok() && has.as_bool() {
                            if let Ok(f) = it.GetCurrent() {
                                if let Some(top) = top_frame_id(f) {
                                    let e = map.entry((browser, top)).or_default();
                                    if !e.contains(&(pid as u32)) {
                                        e.push(pid as u32);
                                    }
                                }
                            }
                            if it.MoveNext(&mut has).is_err() {
                                break;
                            }
                        }
                    }
                }
                // this browser's entries are replaced, the others kept
                let mut all = out.borrow_mut();
                all.retain(|(b, _), _| *b != browser);
                all.extend(map);
                drop(all);
                ready();
                Ok(())
            }));
        let _ = unsafe { env13.GetProcessExtendedInfos(&handler) };
    }

    /// The page got the keyboard focus since the last call.
    pub fn take_got_focus(&self) -> bool {
        self.got_focus.replace(false)
    }

    pub fn take_hotkey(&self) -> Option<Hotkey> {
        self.hotkey.take()
    }

    /// A new document started loading since the last call (first load,
    /// reload, navigation).
    pub fn take_loaded(&self) -> bool {
        self.loaded.take()
    }

    /// A process of the page failed since the last call.
    pub fn take_failure(&self) -> Option<Failure> {
        self.failure.take()
    }
}

/// Ad blocker: checks the page's requests and hides the ad spots once the
/// document is loaded.
unsafe fn watch_requests(webview: &ICoreWebView2, env: &ICoreWebView2Environment) {
    let mut token = Default::default();
    let added = webview.cast::<ICoreWebView2_22>().and_then(|w22| {
        w22.AddWebResourceRequestedFilterWithRequestSourceKinds(
            &HSTRING::from("*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
        )
    });
    if added.is_err() {
        let _ = webview.AddWebResourceRequestedFilter(
            &HSTRING::from("*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        );
    }
    let env = env.clone();
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        let request = args.Request()?;
        let mut uri = PWSTR::null();
        request.Uri(&mut uri)?;
        let uri = take_pwstr(uri);
        let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT_OTHER;
        args.ResourceContext(&mut context)?;
        let mut source = PWSTR::null();
        let _ = sender.Source(&mut source);
        let source = take_pwstr(source);
        use crate::blocker::Kind;
        let kind = match context {
            // the page itself, or a frame inside it
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT if uri == source => Kind::Document,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => Kind::Subdocument,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => Kind::Script,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => Kind::Stylesheet,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => Kind::Image,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => Kind::Font,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => Kind::Media,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST
            | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH
            | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_EVENT_SOURCE => Kind::Xhr,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => Kind::WebSocket,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => Kind::Ping,
            _ => Kind::Other,
        };
        if crate::blocker::check(&uri, &source, kind) {
            let empty = env.CreateWebResourceResponse(
                None,
                403,
                &HSTRING::from("Blocked"),
                &HSTRING::from(""),
            )?;
            args.SetResponse(&empty)?;
        }
        Ok(())
    }));
    let _ = webview.add_WebResourceRequested(&handler, &mut token);

    // cosmetic filters: the page's classes and ids go to the engine, which
    // answers with the CSS hiding the ad spots
    let Ok(w2) = webview.cast::<ICoreWebView2_2>() else {
        return;
    };
    let loaded = DOMContentLoadedEventHandler::create(Box::new(move |sender, _| {
        let Some(sender) = sender else { return Ok(()) };
        let mut source = PWSTR::null();
        let _ = sender.Source(&mut source);
        let source = take_pwstr(source);
        if !source.starts_with("http") {
            return Ok(());
        }
        let page = sender.clone();
        let collect = "(() => { const c = new Set(), i = new Set();             for (const e of document.querySelectorAll('[class],[id]')) {               if (e.id) i.add(e.id); for (const k of e.classList) c.add(k);               if (c.size + i.size > 8000) break; }             return { c: [...c], i: [...i] }; })()";
        let done = ExecuteScriptCompletedHandler::create(Box::new(move |_, json| {
            let v: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
            let list = |k: &str| -> Vec<String> {
                v[k].as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            if let Some(css) = crate::blocker::css_for(&source, &list("c"), &list("i")) {
                let js = format!(
                    "(() => {{ const s = document.createElement('style'); s.textContent = {};                      (document.head || document.documentElement).appendChild(s); }})()",
                    serde_json::Value::String(css)
                );
                let _ = unsafe {
                    page.ExecuteScript(
                        &HSTRING::from(js),
                        &ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(()))),
                    )
                };
            }
            Ok(())
        }));
        let _ = unsafe { sender.ExecuteScript(&HSTRING::from(collect), &done) };
        Ok(())
    }));
    let _ = w2.add_DOMContentLoaded(&loaded, &mut token);
}

/// Page background while it loads (same as the app's panels).
fn background(dark: bool) -> COREWEBVIEW2_COLOR {
    let (r, g, b) = if dark { (24, 24, 27) } else { (244, 244, 245) };
    COREWEBVIEW2_COLOR {
        A: 255,
        R: r,
        G: g,
        B: b,
    }
}

/// Id of the main frame a frame belongs to (iframes and ads included).
fn top_frame_id(frame: ICoreWebView2FrameInfo) -> Option<u32> {
    let mut f = frame.cast::<ICoreWebView2FrameInfo2>().ok()?;
    for _ in 0..64 {
        match unsafe { f.ParentFrameInfo() } {
            Ok(parent) => match parent.cast::<ICoreWebView2FrameInfo2>() {
                Ok(p) => f = p,
                Err(_) => break,
            },
            Err(_) => break,
        }
    }
    let mut id = 0u32;
    unsafe { f.FrameId(&mut id) }.ok()?;
    Some(id)
}

/// Installed WebView2 Runtime version (Err when it is missing).
pub fn runtime_version() -> Result<String, String> {
    let mut p = PWSTR::null();
    unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut p) }
        .map_err(|e| e.to_string())?;
    let v = take_pwstr(p);
    if v.is_empty() {
        Err("WebView2 Runtime not found".into())
    } else {
        Ok(v)
    }
}

pub const RUNTIME_DOWNLOAD_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";
