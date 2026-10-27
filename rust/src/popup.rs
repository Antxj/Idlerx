//! Windows the pages open (pop-ups), hosted by Idlerx itself while the ad
//! blocker is on. Left to WebView2, a pop-up that opens blank and only then
//! goes to an ad could not be checked; here every address it loads is
//! checked, and the window closes before showing anything when it is an
//! ad. The window only appears once it loads something else, so an ad does
//! not flash on the screen.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Once;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    take_pwstr, CreateCoreWebView2ControllerCompletedHandler, DocumentTitleChangedEventHandler,
    NavigationStartingEventHandler, WindowCloseRequestedEventHandler,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows_core::{Interface, BOOL, HSTRING, PWSTR};
use windows_sys::Win32::Foundation as wf;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging as wm;

thread_local! {
    /// The open pop-ups, by window.
    static OPEN: RefCell<HashMap<isize, ICoreWebView2Controller>> = RefCell::new(HashMap::new());
}

const CLASS: &str = "IdlerxPopup";
/// Shown by itself after this long even if it loaded no address (a page
/// written by its opener).
const SHOW_TIMER: usize = 1;
const SHOW_AFTER_MS: u32 = 1500;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS);
        let wc = wm::WNDCLASSW {
            style: wm::CS_HREDRAW | wm::CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            // the app's icon (resource 1)
            hIcon: wm::LoadIconW(instance, 1 as _),
            hCursor: wm::LoadCursorW(std::ptr::null_mut(), wm::IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        wm::RegisterClassW(&wc);
    });
}

/// Hosts the window a page asked for (`args` of its NewWindowRequested):
/// a new page in the same browser profile as the opener (so the same
/// login, InPrivate and VPN), in a window of its own.
pub unsafe fn open(
    opener: &ICoreWebView2,
    env: &ICoreWebView2Environment,
    args: &ICoreWebView2NewWindowRequestedEventArgs,
    account: &str,
) -> windows_core::Result<()> {
    register();
    let mut source = PWSTR::null();
    let _ = opener.Source(&mut source);
    let source = take_pwstr(source);

    // where and how big, as the page asked; else a regular window
    let (mut x, mut y, mut w, mut h) = (wm::CW_USEDEFAULT, wm::CW_USEDEFAULT, 1100, 760);
    if let Ok(f) = args.WindowFeatures() {
        let (mut has_pos, mut has_size) = (BOOL(0), BOOL(0));
        let _ = f.HasPosition(&mut has_pos);
        let _ = f.HasSize(&mut has_size);
        let mut v = [0u32; 4];
        if has_pos.as_bool() && f.Left(&mut v[0]).is_ok() && f.Top(&mut v[1]).is_ok() {
            (x, y) = (v[0] as i32, v[1] as i32);
        }
        if has_size.as_bool() && f.Width(&mut v[2]).is_ok() && f.Height(&mut v[3]).is_ok() {
            (w, h) = ((v[2] as i32).max(200), (v[3] as i32).max(150));
        }
    }
    let class = wide(CLASS);
    let title = wide("Idlerx");
    let hwnd = wm::CreateWindowExW(
        0,
        class.as_ptr(),
        title.as_ptr(),
        wm::WS_OVERLAPPEDWINDOW,
        x,
        y,
        w,
        h,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    );
    if hwnd.is_null() {
        return Err(windows_core::Error::from_thread());
    }
    let key = hwnd as isize;

    // the same profile as the opener: WebView2 requires it
    let profile = opener.cast::<ICoreWebView2_13>()?.Profile()?;
    let mut name = PWSTR::null();
    profile.ProfileName(&mut name)?;
    let name = take_pwstr(name);
    let mut private = BOOL(0);
    let _ = profile.IsInPrivateModeEnabled(&mut private);
    let env10 = env.cast::<ICoreWebView2Environment10>()?;
    let opts = env10.CreateCoreWebView2ControllerOptions()?;
    opts.SetProfileName(&HSTRING::from(name))?;
    opts.SetIsInPrivateModeEnabled(private.as_bool())?;

    let deferral = args.GetDeferral()?;
    let args = args.clone();
    let env2 = env.clone();
    let account = account.to_string();
    let done =
        CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |res, controller| {
            let ready = res.and_then(|_| {
                let controller = controller.ok_or_else(windows_core::Error::empty)?;
                let webview = controller.CoreWebView2()?;
                setup(key, &controller, &webview, &env2, &source, &account)?;
                args.SetNewWindow(&webview)?;
                OPEN.with(|o| o.borrow_mut().insert(key, controller));
                Ok(())
            });
            if ready.is_err() {
                // no window at all rather than a broken one
                let _ = args.SetHandled(true);
                wm::DestroyWindow(key as _);
            } else {
                wm::SetTimer(key as _, SHOW_TIMER, SHOW_AFTER_MS, None);
            }
            let _ = deferral.Complete();
            Ok(())
        }));
    if let Err(e) = env10.CreateCoreWebView2ControllerWithOptions(HWND(hwnd), &opts, &done) {
        wm::DestroyWindow(hwnd);
        return Err(e);
    }
    Ok(())
}

unsafe fn setup(
    key: isize,
    controller: &ICoreWebView2Controller,
    webview: &ICoreWebView2,
    env: &ICoreWebView2Environment,
    source: &str,
    account: &str,
) -> windows_core::Result<()> {
    let mut token = Default::default();
    fit(key, controller);
    if let Ok(settings) = webview.Settings() {
        let _ = settings.SetIsStatusBarEnabled(false);
        let _ = settings.SetAreDevToolsEnabled(cfg!(debug_assertions));
    }
    // each address it goes to: an ad closes the window
    let (opener, name) = (source.to_string(), account.to_string());
    let navigation = NavigationStartingEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut uri = PWSTR::null();
        args.Uri(&mut uri)?;
        let uri = take_pwstr(uri);
        if crate::blocker::popup_navigation_blocked(&uri, &opener) {
            args.SetCancel(true)?;
            let shown: String = uri.chars().take(150).collect();
            crate::events::log(&name, &format!("janela de propaganda bloqueada: {shown}"));
            wm::PostMessageW(key as _, wm::WM_CLOSE, 0, 0);
        } else if uri.starts_with("http") {
            show(key);
        }
        Ok(())
    }));
    webview.add_NavigationStarting(&navigation, &mut token)?;
    // window.close() of the page
    let close = WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
        wm::PostMessageW(key as _, wm::WM_CLOSE, 0, 0);
        Ok(())
    }));
    webview.add_WindowCloseRequested(&close, &mut token)?;
    let title = DocumentTitleChangedEventHandler::create(Box::new(move |sender, _| {
        if let Some(sender) = sender {
            let mut t = PWSTR::null();
            sender.DocumentTitle(&mut t)?;
            let t = take_pwstr(t);
            let text = wide(if t.is_empty() { "Idlerx" } else { &t });
            wm::SetWindowTextW(key as _, text.as_ptr());
        }
        Ok(())
    }));
    webview.add_DocumentTitleChanged(&title, &mut token)?;
    // the blocker inside it too, and the windows it opens
    crate::panels::watch_requests(webview, env, account);
    Ok(())
}

/// The page fills the window.
fn fit(key: isize, controller: &ICoreWebView2Controller) {
    let mut r = wf::RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        wm::GetClientRect(key as _, &mut r);
        let _ = controller.SetBounds(RECT {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        });
    }
}

fn show(key: isize) {
    unsafe {
        wm::KillTimer(key as _, SHOW_TIMER);
        if wm::IsWindowVisible(key as _) == 0 {
            wm::ShowWindow(key as _, wm::SW_SHOWNORMAL);
            wm::SetForegroundWindow(key as _);
        }
    }
    OPEN.with(|o| {
        if let Some(c) = o.borrow().get(&key) {
            let _ = unsafe { c.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
        }
    });
}

unsafe extern "system" fn window_proc(
    hwnd: wf::HWND,
    msg: u32,
    wparam: wf::WPARAM,
    lparam: wf::LPARAM,
) -> wf::LRESULT {
    let key = hwnd as isize;
    match msg {
        wm::WM_SIZE => {
            OPEN.with(|o| {
                if let Some(c) = o.borrow().get(&key) {
                    fit(key, c);
                }
            });
            0
        }
        wm::WM_TIMER if wparam == SHOW_TIMER => {
            show(key);
            0
        }
        wm::WM_SETFOCUS => {
            OPEN.with(|o| {
                if let Some(c) = o.borrow().get(&key) {
                    let _ = c.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
                }
            });
            0
        }
        wm::WM_DESTROY => {
            if let Some(c) = OPEN.with(|o| o.borrow_mut().remove(&key)) {
                let _ = c.Close();
            }
            0
        }
        _ => wm::DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
