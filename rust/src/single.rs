//! Single instance: named mutex + named event for "show yourself".
//!
//! Two instances would open the same browser profiles at the same time, so
//! the second one only asks the first one to show up and exits.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicIsize, Ordering};

const ERROR_ALREADY_EXISTS: u32 = 183;
const EVENT_MODIFY_STATE: u32 = 0x0002;
const WAIT_OBJECT_0: u32 = 0;
const SW_RESTORE: i32 = 9;

#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(attrs: *mut c_void, initial_owner: i32, name: *const u16) -> *mut c_void;
    fn CreateEventW(
        attrs: *mut c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> *mut c_void;
    fn OpenEventW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
    fn SetEvent(handle: *mut c_void) -> i32;
    fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
    fn GetLastError() -> u32;
}

#[link(name = "user32")]
extern "system" {
    fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    fn IsIconic(hwnd: isize) -> i32;
    fn SetForegroundWindow(hwnd: isize) -> i32;
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

const MUTEX_NAME: &str = "Local\\idlerx-single-instance";
const EVENT_NAME: &str = "Local\\idlerx-show";

/// Main window, registered by the interface once it exists.
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);

pub fn set_main_window(hwnd: isize) {
    MAIN_WINDOW.store(hwnd, Ordering::SeqCst);
}

/// Windows object name. IDLERX_INSTANCE (tests only) separates a test
/// instance from the everyday app open in the same session.
fn nome(base: &str) -> Vec<u16> {
    match std::env::var("IDLERX_INSTANCE") {
        Ok(sufixo) if !sufixo.is_empty() => wide(&format!("{base}-{sufixo}")),
        _ => wide(base),
    }
}

/// Returns true if this is the first instance (and keeps the mutex alive for
/// the rest of the process). If another one exists, signals it and returns false.
pub fn acquire_or_signal() -> bool {
    let name = nome(MUTEX_NAME);
    let handle = unsafe { CreateMutexW(ptr::null_mut(), 0, name.as_ptr()) };
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    // handle intentionally "leaked": lives until the process ends
    let _ = handle;
    if !already {
        return true;
    }
    let ev_name = nome(EVENT_NAME);
    let ev = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, ev_name.as_ptr()) };
    if !ev.is_null() {
        unsafe { SetEvent(ev) };
    }
    false
}

/// Creates the "show yourself" event and a thread that restores the main
/// window whenever a second instance signals.
pub fn spawn_show_listener() {
    let ev_name = nome(EVENT_NAME);
    let ev = unsafe { CreateEventW(ptr::null_mut(), 0, 0, ev_name.as_ptr()) } as usize;
    if ev == 0 {
        return;
    }
    std::thread::spawn(move || loop {
        let r = unsafe { WaitForSingleObject(ev as *mut c_void, 60_000) };
        if r == WAIT_OBJECT_0 {
            show_main_window();
        }
    });
}

pub fn show_main_window() {
    let hwnd = MAIN_WINDOW.load(Ordering::SeqCst);
    if hwnd == 0 {
        return;
    }
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
}
