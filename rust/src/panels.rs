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
    FocusChangedEventHandler, GetProcessExtendedInfosCompletedHandler,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows_core::{Interface, BOOL, HSTRING, PCWSTR, PWSTR};

/// WebView2 browser arguments. With `keep_active`, hidden or covered pages
/// keep their timers at full speed (idle games keep playing while another
/// account is on screen).
fn browser_args(keep_active: bool) -> String {
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
    s
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

/// Result of [`open`], filled in when the page exists (or failed).
pub type PanelSlot = Rc<RefCell<Option<Result<Panel, String>>>>;

/// Wakes the interface up (a repaint) from a WebView2 callback.
pub type Wake = Rc<dyn Fn()>;

/// Starts opening an account inside `parent` (the main window). `ready` is
/// called on the UI thread when the slot is filled.
pub fn open(
    parent: isize,
    account: &Account,
    keep_active: bool,
    economy: bool,
    save_passwords: bool,
    dark: bool,
    wake: Wake,
) -> PanelSlot {
    let slot: PanelSlot = Rc::new(RefCell::new(None));
    let dir = if economy {
        config::shared_dir()
    } else {
        config::profile_dir(&account.profile)
    };
    let _ = std::fs::create_dir_all(&dir);
    let options = CoreWebView2EnvironmentOptions::default();
    unsafe { options.set_additional_browser_arguments(browser_args(keep_active)) };

    let s = slot.clone();
    let account = account.clone();
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
                if economy {
                    // the account is a named profile inside the shared browser
                    env.cast::<ICoreWebView2Environment10>().and_then(|env10| {
                        let opts = env10.CreateCoreWebView2ControllerOptions()?;
                        opts.SetProfileName(&HSTRING::from(profile.as_str()))?;
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
            let r = fps_resend.clone();
            let w = wake.clone();
            let loading = ContentLoadingEventHandler::create(Box::new(move |_, _| {
                r.set(true);
                w();
                Ok(())
            }));
            let _ = webview.add_ContentLoading(&loading, &mut token);
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
    pub fn place(&mut self, rect: Option<egui::Rect>, pixels_per_point: f32) {
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
                }
            }
            _ => {
                if self.visible {
                    self.visible = false;
                    // hidden: stops drawing and asks WebView2 to keep less
                    // memory (the page keeps running)
                    let _ = unsafe { self.controller.SetIsVisible(false) };
                    self.memory_level(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
                }
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
    pub fn set_fps(&self, fps: u32) {
        if !self.fps_resend.replace(false) && self.fps_sent.get() == Some(fps) {
            return;
        }
        self.fps_sent.set(Some(fps));
        let js = format!("window.__idlerxFps && window.__idlerxFps.set({fps})");
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
    pub fn request_renderers(
        &self,
        out: Rc<RefCell<HashMap<u32, Vec<u32>>>>,
        ready: impl Fn() + 'static,
    ) {
        let Ok(env13) = self.env.cast::<ICoreWebView2Environment13>() else {
            return;
        };
        let handler =
            GetProcessExtendedInfosCompletedHandler::create(Box::new(move |res, list| {
                if res.is_err() {
                    return Ok(());
                }
                let Some(list) = list else { return Ok(()) };
                let mut map: HashMap<u32, Vec<u32>> = HashMap::new();
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
                                    let e = map.entry(top).or_default();
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
                *out.borrow_mut() = map;
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
