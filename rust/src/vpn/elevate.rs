//! Administrator rights on demand. Idlerx runs as a regular user; only a few
//! occasional tasks need elevation (installing OpenVPN, creating an extra
//! network adapter, authorizing the user for the OpenVPN service). Each one
//! shows the Windows UAC prompt at that moment.

use std::path::Path;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Runs `exe params` as administrator (UAC prompt), waits for it and returns
/// its exit code. Err with a readable message if the user declines.
pub fn run_as_admin(exe: &Path, params: &str) -> Result<u32, String> {
    let verb = wide("runas");
    let file = wide(&exe.to_string_lossy());
    let params = wide(params);
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = SW_HIDE;

    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(match unsafe { GetLastError() } {
            ERROR_CANCELLED => tr!(
                "a permissão de administrador foi recusada",
                "the administrator permission was declined"
            )
            .to_string(),
            e => trf!(
                "não foi possível executar como administrador (erro {e})",
                "could not run as administrator (error {e})"
            ),
        });
    }
    if info.hProcess.is_null() {
        return Ok(0);
    }
    let mut code = 0u32;
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
    }
    Ok(code)
}
