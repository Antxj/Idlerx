//! The account's Firefox window inside Idlerx's: found by the marker the
//! extension puts in its title, turned into a child window of the main one,
//! and given clicks and keys as window messages (Firefox takes them as real
//! input; the Windows mouse and keyboard are not used, and it works with the
//! account hidden).

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, SetFocus, MAPVK_VK_TO_VSC};
use windows_sys::Win32::UI::WindowsAndMessaging as wm;

/// Window of a Firefox page (top-level, or already embedded).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window(pub isize);

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { wm::GetClassNameW(h, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn title_of(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { wm::GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The Firefox window whose title has `marker` (top-level, or already a
/// child of `parent`).
pub fn find(marker: &str, parent: isize) -> Option<Window> {
    struct Search<'a> {
        marker: &'a str,
        found: HWND,
    }
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> i32 {
        let s = &mut *(l as *mut Search);
        if class_of(h) == "MozillaWindowClass" && title_of(h).contains(s.marker) {
            s.found = h;
            return 0;
        }
        1
    }
    let mut s = Search {
        marker,
        found: std::ptr::null_mut(),
    };
    unsafe {
        wm::EnumWindows(Some(each), &mut s as *mut _ as LPARAM);
        if s.found.is_null() {
            wm::EnumChildWindows(parent as HWND, Some(each), &mut s as *mut _ as LPARAM);
        }
    }
    (!s.found.is_null()).then_some(Window(s.found as isize))
}

impl Window {
    fn h(self) -> HWND {
        self.0 as HWND
    }

    /// The window still exists and is not embedded.
    pub fn exists_top_level(self) -> bool {
        unsafe { wm::IsWindow(self.h()) != 0 && wm::GetParent(self.h()).is_null() }
    }

    /// Process of the window (Firefox's main process).
    pub fn pid(self) -> u32 {
        let mut pid = 0u32;
        unsafe { wm::GetWindowThreadProcessId(self.h(), &mut pid) };
        pid
    }

    /// Turns the window into a hidden child of `parent`, without frame.
    pub fn embed(self, parent: isize) {
        unsafe {
            let h = self.h();
            wm::ShowWindow(h, wm::SW_HIDE);
            let style = wm::GetWindowLongW(h, wm::GWL_STYLE) as u32;
            let style = (style
                & !(wm::WS_POPUP
                    | wm::WS_CAPTION
                    | wm::WS_THICKFRAME
                    | wm::WS_MINIMIZEBOX
                    | wm::WS_MAXIMIZEBOX
                    | wm::WS_SYSMENU
                    | wm::WS_MINIMIZE
                    | wm::WS_MAXIMIZE))
                | wm::WS_CHILD
                | wm::WS_CLIPSIBLINGS;
            wm::SetWindowLongW(h, wm::GWL_STYLE, style as i32);
            let ex = wm::GetWindowLongW(h, wm::GWL_EXSTYLE) as u32;
            let ex = ex & !(wm::WS_EX_APPWINDOW | wm::WS_EX_WINDOWEDGE | wm::WS_EX_DLGMODALFRAME);
            wm::SetWindowLongW(h, wm::GWL_EXSTYLE, ex as i32);
            wm::SetParent(h, parent as HWND);
            // a window created minimized: as a child it comes back normal
            wm::ShowWindow(h, wm::SW_RESTORE);
            wm::ShowWindow(h, wm::SW_HIDE);
        }
    }

    /// Puts the window on `b` (left, top, right, bottom in the parent's
    /// pixels) and shows it.
    pub fn show_at(self, b: (i32, i32, i32, i32)) {
        unsafe {
            wm::SetWindowPos(
                self.h(),
                wm::HWND_TOP,
                b.0,
                b.1,
                (b.2 - b.0).max(1),
                (b.3 - b.1).max(1),
                wm::SWP_NOACTIVATE | wm::SWP_SHOWWINDOW | wm::SWP_FRAMECHANGED,
            );
        }
    }

    pub fn visible(self) -> bool {
        unsafe { wm::IsWindowVisible(self.h()) != 0 }
    }

    pub fn hide(self) {
        unsafe { wm::ShowWindow(self.h(), wm::SW_HIDE) };
    }

    pub fn focus(self) {
        unsafe { SetFocus(self.h()) };
    }

    /// Size of the page area, in pixels.
    pub fn client_size(self) -> (i32, i32) {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        unsafe { wm::GetClientRect(self.h(), &mut r) };
        (r.right - r.left, r.bottom - r.top)
    }

    fn post(self, msg: u32, w: usize, l: isize) {
        unsafe { wm::PostMessageW(self.h(), msg, w as WPARAM, l as LPARAM) };
    }

    /// A click at a point given as a fraction of the page's width and
    /// height. False when the window has no size yet.
    pub fn click(self, fx: f32, fy: f32, right: bool, double: bool) -> bool {
        let (w, h) = self.client_size();
        if w < 2 || h < 2 {
            return false;
        }
        let x = (fx.clamp(0.0, 1.0) * (w - 1) as f32).round() as i32;
        let y = (fy.clamp(0.0, 1.0) * (h - 1) as f32).round() as i32;
        let pos = ((y as u32 & 0xFFFF) << 16 | (x as u32 & 0xFFFF)) as isize;
        let (down, up, dbl, mk) = if right {
            (
                wm::WM_RBUTTONDOWN,
                wm::WM_RBUTTONUP,
                wm::WM_RBUTTONDBLCLK,
                0x0002,
            )
        } else {
            (
                wm::WM_LBUTTONDOWN,
                wm::WM_LBUTTONUP,
                wm::WM_LBUTTONDBLCLK,
                0x0001,
            )
        };
        self.post(wm::WM_MOUSEMOVE, 0, pos);
        self.post(down, mk, pos);
        self.post(up, 0, pos);
        if double {
            self.post(dbl, mk, pos);
            self.post(up, 0, pos);
        }
        true
    }

    /// Makes the window the one its Firefox holds active (where it types).
    pub fn activate(self) {
        self.post(wm::WM_ACTIVATE, 1, 0);
        self.post(wm::WM_SETFOCUS, 0, 0);
    }

    /// Presses and releases a key (virtual key code, and its text if it
    /// types something).
    pub fn key(self, vk: u16, text: Option<&str>) {
        // the scan code gives the page the key's position (`code`)
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as isize;
        // arrows, Insert/Delete, Home/End, PageUp/Down: "extended" keys
        let extended = matches!(vk, 0x21..=0x28 | 0x2D | 0x2E) as isize;
        let l_down = 1 | (scan << 16) | (extended << 24);
        let l_up = l_down | (1 << 30) | (1 << 31);
        // The accounts of a network share one Firefox, which gives keys to
        // the window it holds active, not to the one that got the message:
        // this window is made the active one for the key, then the window
        // that really has the keyboard gets it back (all in the order of
        // Firefox's queue), so the user's own typing stays where it was.
        let focused = keyboard_window(self);
        self.post(wm::WM_ACTIVATE, 1, 0);
        self.post(wm::WM_SETFOCUS, 0, 0);
        self.post(wm::WM_KEYDOWN, vk as usize, l_down);
        if let Some(t) = text {
            for c in t.encode_utf16() {
                self.post(wm::WM_CHAR, c as usize, l_down);
            }
        }
        self.post(wm::WM_KEYUP, vk as usize, l_up);
        self.post(wm::WM_KILLFOCUS, 0, 0);
        self.post(wm::WM_ACTIVATE, 0, 0);
        if let Some(f) = focused.filter(|f| *f != self) {
            f.post(wm::WM_ACTIVATE, 1, 0);
            f.post(wm::WM_SETFOCUS, 0, 0);
        }
    }

    /// The window Firefox opens when it starts: hidden for good, without a
    /// taskbar button. It keeps that Firefox running while it has no account.
    pub fn make_keeper(self) {
        unsafe {
            let h = self.h();
            wm::ShowWindow(h, wm::SW_HIDE);
            let ex = wm::GetWindowLongW(h, wm::GWL_EXSTYLE) as u32;
            let ex = (ex | wm::WS_EX_TOOLWINDOW) & !wm::WS_EX_APPWINDOW;
            wm::SetWindowLongW(h, wm::GWL_EXSTYLE, ex as i32);
        }
    }

    /// Asks the window to close (Firefox closes the page normally).
    pub fn close(self) {
        self.post(wm::WM_CLOSE, 0, 0);
    }
}

/// Top-level Firefox windows of a process: all of them, or only the visible
/// ones that are not an account's (`only_strays`).
pub fn windows_of(pid: u32, only_strays: bool) -> Vec<Window> {
    struct Search {
        pid: u32,
        strays: bool,
        out: Vec<Window>,
    }
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> i32 {
        let s = &mut *(l as *mut Search);
        let mut pid = 0u32;
        wm::GetWindowThreadProcessId(h, &mut pid);
        if pid == s.pid
            && class_of(h) == "MozillaWindowClass"
            && (!s.strays
                || (wm::IsWindowVisible(h) != 0 && !title_of(h).contains("\u{2063}idlerx:")))
        {
            s.out.push(Window(h as isize));
        }
        1
    }
    let mut s = Search {
        pid,
        strays: only_strays,
        out: Vec::new(),
    };
    unsafe { wm::EnumWindows(Some(each), &mut s as *mut _ as LPARAM) };
    s.out
}

/// The account window of Firefox's thread that has the keyboard (None if
/// none).
fn keyboard_window(of: Window) -> Option<Window> {
    unsafe {
        let thread = wm::GetWindowThreadProcessId(of.h(), std::ptr::null_mut());
        let mut info: wm::GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<wm::GUITHREADINFO>() as u32;
        if wm::GetGUIThreadInfo(thread, &mut info) == 0 || info.hwndFocus.is_null() {
            return None;
        }
        // focus may be on a child of the account's window: the account's is
        // the Firefox window embedded in Idlerx's (its parent is not Firefox's)
        let mut h = info.hwndFocus;
        loop {
            let parent = wm::GetParent(h);
            if parent.is_null() {
                return None;
            }
            if class_of(h) == "MozillaWindowClass" && class_of(parent) != "MozillaWindowClass" {
                return Some(Window(h as isize));
            }
            h = parent;
        }
    }
}

// ------------------------------------------------------------------ focus --

thread_local! {
    /// Windows that got the keyboard focus (see `watch_focus`).
    static FOCUSED: std::cell::RefCell<Vec<isize>> = const { std::cell::RefCell::new(Vec::new()) };
}
static WAKE: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> = std::sync::OnceLock::new();

/// Tells Idlerx when a Firefox window gets the focus (the user clicked in
/// an account), whatever its page: Windows reports it (a blank page has no
/// extension script to do it).
pub fn watch_focus(wake: impl Fn() + Send + Sync + 'static) {
    use windows_sys::Win32::UI::Accessibility::SetWinEventHook;
    let _ = WAKE.set(Box::new(wake));
    unsafe {
        SetWinEventHook(
            wm::EVENT_OBJECT_FOCUS,
            wm::EVENT_OBJECT_FOCUS,
            std::ptr::null_mut(),
            Some(focus_event),
            0,
            0,
            wm::WINEVENT_OUTOFCONTEXT,
        );
    }
    // windows that show are seen on a thread of their own: Idlerx's own
    // thread may be busy drawing, and they must be hidden at once
    std::thread::spawn(|| unsafe {
        SetWinEventHook(
            wm::EVENT_OBJECT_SHOW,
            wm::EVENT_OBJECT_SHOW,
            std::ptr::null_mut(),
            Some(show_event),
            0,
            0,
            wm::WINEVENT_OUTOFCONTEXT,
        );
        let mut msg: wm::MSG = std::mem::zeroed();
        while wm::GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            wm::DispatchMessageW(&msg);
        }
    });
}

unsafe extern "system" fn show_event(
    _hook: windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if object == wm::OBJID_WINDOW && !hwnd.is_null() && class_of(hwnd) == "MozillaWindowClass" {
        shown(hwnd);
    }
}

unsafe extern "system" fn focus_event(
    _hook: windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if hwnd.is_null() || class_of(hwnd) != "MozillaWindowClass" {
        return;
    }
    FOCUSED.with(|f| f.borrow_mut().push(hwnd as isize));
    if let Some(w) = WAKE.get() {
        w();
    }
}

/// Firefox windows that got the focus since the last call.
pub fn take_focused() -> Vec<Window> {
    FOCUSED
        .with(|f| std::mem::take(&mut *f.borrow_mut()))
        .into_iter()
        .map(Window)
        .collect()
}

impl Window {
    /// This window, or one of its parents, is `account`.
    pub fn within(self, account: Window) -> bool {
        let mut h = self.h();
        for _ in 0..8 {
            if h.is_null() {
                return false;
            }
            if h as isize == account.0 {
                return true;
            }
            h = unsafe { wm::GetParent(h) };
        }
        false
    }
}

// ------------------------------------------------------------- held back --

/// The job of Idlerx's Firefoxes, while one of them starts or opens
/// accounts: their new windows are held back (see `guard`).
static GUARDED: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
/// Windows held back, and since when.
static HELD: std::sync::Mutex<Vec<(isize, std::time::Instant)>> = std::sync::Mutex::new(Vec::new());

/// While a Firefox starts or opens accounts (`job` is the job its processes
/// are in; 0: none does), new top-level windows of Idlerx's Firefoxes are
/// hidden the moment they show (Firefox shows its start window, and creates
/// each account's window on the screen before minimizing it): nothing
/// flashes outside Idlerx.
pub fn guard(job: isize) {
    GUARDED.store(job, std::sync::atomic::Ordering::Relaxed);
}

/// Windows held back since the last call, with when.
pub fn take_held() -> Vec<(Window, std::time::Instant)> {
    std::mem::take(&mut *HELD.lock().unwrap())
        .into_iter()
        .map(|(w, t)| (Window(w), t))
        .collect()
}

unsafe fn shown(hwnd: HWND) {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    let job = GUARDED.load(std::sync::atomic::Ordering::Relaxed);
    // only top-level windows (embedded accounts are children)
    if job == 0 || !wm::GetParent(hwnd).is_null() {
        return;
    }
    let mut pid = 0u32;
    wm::GetWindowThreadProcessId(hwnd, &mut pid);
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if process.is_null() {
        return;
    }
    let mut ours = 0;
    windows_sys::Win32::System::JobObjects::IsProcessInJob(process, job as _, &mut ours);
    windows_sys::Win32::Foundation::CloseHandle(process);
    if ours == 0 {
        return;
    }
    wm::ShowWindow(hwnd, wm::SW_HIDE);
    HELD.lock()
        .unwrap()
        .push((hwnd as isize, std::time::Instant::now()));
    if let Some(w) = WAKE.get() {
        w();
    }
}

impl Window {
    /// The window has an account's marker in its title.
    pub fn is_account(self) -> bool {
        title_of(self.h()).contains("\u{2063}idlerx:")
    }

    /// Shows a window that was held back but is not an account's (a window
    /// a page opened), without taking the focus away.
    pub fn release(self) {
        unsafe {
            if wm::GetParent(self.h()).is_null() {
                wm::ShowWindow(self.h(), wm::SW_SHOWNOACTIVATE);
            }
        }
    }
}
