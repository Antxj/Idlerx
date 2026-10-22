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

    pub fn exists(self) -> bool {
        unsafe { wm::IsWindow(self.h()) != 0 }
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
            (wm::WM_RBUTTONDOWN, wm::WM_RBUTTONUP, wm::WM_RBUTTONDBLCLK, 0x0002)
        } else {
            (wm::WM_LBUTTONDOWN, wm::WM_LBUTTONUP, wm::WM_LBUTTONDBLCLK, 0x0001)
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

    /// Presses and releases a key (virtual key code, and its text if it
    /// types something).
    pub fn key(self, vk: u16, text: Option<&str>) {
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as isize;
        // arrows, Insert/Delete, Home/End, PageUp/Down: "extended" keys
        let extended = matches!(vk, 0x21..=0x28 | 0x2D | 0x2E) as isize;
        let l_down = 1 | (scan << 16) | (extended << 24);
        let l_up = l_down | (1 << 30) | (1 << 31);
        // the page must have the keyboard focus inside the window
        self.post(wm::WM_SETFOCUS, 0, 0);
        self.post(wm::WM_KEYDOWN, vk as usize, l_down);
        if let Some(t) = text {
            for c in t.encode_utf16() {
                self.post(wm::WM_CHAR, c as usize, l_down);
            }
        }
        self.post(wm::WM_KEYUP, vk as usize, l_up);
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
