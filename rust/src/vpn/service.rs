//! Client of the OpenVPN interactive service (OpenVPNServiceInteractive).
//!
//! The service runs as SYSTEM and starts openvpn.exe on behalf of a regular
//! user, performing the privileged network changes (adapter address, routes)
//! itself and undoing them when OpenVPN exits. This is what lets Idlerx run
//! without administrator rights - OpenVPN GUI works the same way.
//!
//! Protocol (openvpnserv/interactive.c, OpenVPN 2.4+):
//! - the client writes the UTF-16 string "workdir\0options\0stdin\0" to the
//!   pipe \\.\pipe\openvpn\service;
//! - the service answers "0x00000000\n0x<pid>\nProcess ID" on success, or
//!   "0x<error>\n<function>\n<message>" on failure.
//!
//! Users who are neither administrators nor members of the group configured
//! in the registry (default "OpenVPN Administrators") may only start configs
//! from the global config folder, with a short whitelist of options. Idlerx
//! offers to add the user to that group once (an administrator approves it).

use super::elevate::wide;
use std::ffi::c_void;
use std::path::Path;
use std::ptr;
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_PIPE_BUSY,
    GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, OPEN_EXISTING};
use windows_sys::Win32::System::Pipes::{
    SetNamedPipeHandleState, WaitNamedPipeW, PIPE_READMODE_MESSAGE,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
    WaitForSingleObject, PROCESS_NAME_WIN32, PROCESS_QUERY_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

const PIPE: &str = r"\\.\pipe\openvpn\service";
const DEFAULT_ADMIN_GROUP: &str = "OpenVPN Administrators";
/// Command line argument of the elevated helper that authorizes a user.
pub const ARG_AUTHORIZE: &str = "--authorize-openvpn-user";

/// Why OpenVPN could not be started through the service.
#[derive(Debug, PartialEq)]
pub enum Failure {
    /// The service is not installed or not running.
    Unavailable,
    /// The user needs to be authorized (options outside the whitelist).
    NotAuthorized(String),
    Other(String),
}

impl Failure {
    pub fn message(&self) -> String {
        match self {
            Failure::Unavailable => tr!(
                "O serviço do OpenVPN (OpenVPNServiceInteractive) não está rodando. \
                 Reinstale o OpenVPN Community ou reinicie o computador.",
                "The OpenVPN service (OpenVPNServiceInteractive) is not running. \
                 Reinstall OpenVPN Community or restart the computer."
            )
            .into(),
            Failure::NotAuthorized(m) | Failure::Other(m) => m.clone(),
        }
    }
}

/// A handle closed on Drop.
struct Handle(HANDLE);
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// openvpn.exe started by the service. The user's token can wait for it,
/// read its exit code and terminate it (the service grants exactly that).
pub struct Process {
    pub pid: u32,
    process: Handle,
    _pipe: Handle,
}

impl Process {
    /// Exit code, if the process has ended.
    pub fn exited(&self) -> Option<u32> {
        if self.process.0.is_null() {
            return Some(u32::MAX); // ended before it could be opened
        }
        if unsafe { WaitForSingleObject(self.process.0, 0) } != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.process.0, &mut code) };
        Some(code)
    }

    /// Waits up to `limit` for the process to end; kills it afterwards.
    pub fn wait(&self, limit: Duration) -> Option<u32> {
        if !self.process.0.is_null() {
            unsafe { WaitForSingleObject(self.process.0, limit.as_millis() as u32) };
        }
        if self.exited().is_none() {
            self.kill();
            std::thread::sleep(Duration::from_millis(500));
        }
        self.exited()
    }

    pub fn kill(&self) {
        if !self.process.0.is_null() {
            unsafe { TerminateProcess(self.process.0, 1) };
        }
    }
}

/// Quotes one argument the way CommandLineToArgvW (used by the service to
/// validate the options) parses it back.
pub fn arg(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"']) {
        return a.to_string();
    }
    let mut out = String::from('"');
    let mut slashes = 0;
    for c in a.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(slashes));
                out.push(c);
                slashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

/// Reads the service answer: Ok(pid) or Err((code, message)).
fn parse_answer(text: &str) -> Result<u32, (u32, String)> {
    let mut lines = text.trim_end_matches('\0').splitn(3, '\n');
    let hex = |s: &str| u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok();
    let code = lines
        .next()
        .and_then(hex)
        .ok_or((u32::MAX, text.to_string()))?;
    let line2 = lines.next().unwrap_or_default().trim().to_string();
    let line3 = lines.next().unwrap_or_default().trim().to_string();
    if code == 0 {
        return hex(&line2).ok_or((u32::MAX, text.to_string()));
    }
    let msg = if line3.is_empty() {
        line2
    } else {
        format!("{line3} ({line2})")
    };
    Err((code, msg))
}

fn open_pipe(name: &[u16]) -> HANDLE {
    unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    }
}

/// Starts openvpn.exe through the service, in `workdir`, with `options`
/// (already quoted with `arg`).
pub fn start(workdir: &Path, options: &str) -> Result<Process, Failure> {
    let name = wide(PIPE);
    let mut pipe = open_pipe(&name);
    if pipe == INVALID_HANDLE_VALUE {
        match unsafe { GetLastError() } {
            ERROR_PIPE_BUSY => {
                // another client is talking to the service right now
                unsafe { WaitNamedPipeW(name.as_ptr(), 5000) };
                pipe = open_pipe(&name);
            }
            ERROR_FILE_NOT_FOUND => return Err(Failure::Unavailable),
            e => return Err(Failure::Other(format!("CreateFile({PIPE}): {e}"))),
        }
        if pipe == INVALID_HANDLE_VALUE {
            return Err(Failure::Unavailable);
        }
    }
    let pipe = Handle(pipe);
    let mode = PIPE_READMODE_MESSAGE;
    unsafe { SetNamedPipeHandleState(pipe.0, &mode, ptr::null(), ptr::null()) };

    // workdir \0 options \0 stdin (empty) \0
    let mut data: Vec<u16> = workdir.to_string_lossy().encode_utf16().collect();
    data.push(0);
    data.extend(options.encode_utf16());
    data.push(0);
    data.push(0);
    let mut written = 0u32;
    let ok = unsafe {
        WriteFile(
            pipe.0,
            data.as_ptr() as *const u8,
            (data.len() * 2) as u32,
            &mut written,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(Failure::Other(format!("WriteFile: {}", unsafe {
            GetLastError()
        })));
    }

    let mut buf = [0u16; 2048];
    let mut read = 0u32;
    let ok = unsafe {
        ReadFile(
            pipe.0,
            buf.as_mut_ptr() as *mut u8,
            (buf.len() * 2) as u32,
            &mut read,
            ptr::null_mut(),
        )
    };
    if ok == 0 && unsafe { GetLastError() } != ERROR_MORE_DATA {
        return Err(Failure::Other(format!("ReadFile: {}", unsafe {
            GetLastError()
        })));
    }
    let answer = String::from_utf16_lossy(&buf[..(read as usize / 2)]);
    match parse_answer(&answer) {
        Ok(pid) => {
            // exactly the rights the service grants the user on that process
            let access = PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_INFORMATION;
            let process = Handle(unsafe { OpenProcess(access, 0, pid) });
            Ok(Process {
                pid,
                process,
                _pipe: pipe,
            })
        }
        Err((_, msg)) if msg.contains("admin approval") => Err(Failure::NotAuthorized(msg)),
        Err((code, msg)) => Err(Failure::Other(format!("0x{code:08x}: {msg}"))),
    }
}

/// Terminates an openvpn.exe left behind by a previous run of Idlerx (killed
/// or crashed). Checks the image name, since PIDs are reused.
pub fn kill_if_openvpn(pid: u32) -> bool {
    let h = Handle(unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    });
    if h.0.is_null() {
        return false;
    }
    let mut name = [0u16; 1024];
    let mut size = name.len() as u32;
    if unsafe { QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, name.as_mut_ptr(), &mut size) }
        == 0
    {
        return false;
    }
    let path = String::from_utf16_lossy(&name[..size as usize]).to_lowercase();
    if !path.ends_with("\\openvpn.exe") {
        return false;
    }
    unsafe { TerminateProcess(h.0, 1) != 0 }
}

// ------------------------------------------------- user authorization --

/// Group whose members may use any config/option (registry ovpn_admin_group).
pub fn authorized_group() -> String {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
    };
    let key = wide(r"SOFTWARE\OpenVPN");
    let value = wide("ovpn_admin_group");
    let mut buf = [0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            ptr::null_mut(),
            buf.as_mut_ptr() as *mut c_void,
            &mut len,
        )
    };
    if ok == 0 {
        let n = (len as usize / 2).saturating_sub(1).min(buf.len());
        let name = String::from_utf16_lossy(&buf[..n]);
        if !name.trim().is_empty() {
            return name;
        }
    }
    DEFAULT_ADMIN_GROUP.to_string()
}

/// Current Windows user as DOMAIN\name (or COMPUTER\name for local accounts).
fn current_user() -> String {
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    let name = std::env::var("USERNAME").unwrap_or_default();
    if domain.is_empty() {
        name
    } else {
        format!("{domain}\\{name}")
    }
}

/// Asks Windows for administrator approval (UAC) to add the current user to
/// the authorized group. Runs this same executable elevated with ARG_AUTHORIZE.
pub fn authorize_current_user() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let params = format!("{ARG_AUTHORIZE} {}", arg(&current_user()));
    match super::elevate::run_as_admin(&exe, &params)? {
        0 => Ok(()),
        code => Err(trf!(
            "não foi possível autorizar o usuário (código {code})",
            "could not authorize the user (code {code})"
        )),
    }
}

/// Elevated side of `authorize_current_user`: creates the group if needed
/// and adds the user to it. Returns the process exit code.
pub fn add_to_group(user: &str) -> i32 {
    use windows_sys::Win32::NetworkManagement::NetManagement::{
        NetLocalGroupAdd, NetLocalGroupAddMembers, LOCALGROUP_INFO_1, LOCALGROUP_MEMBERS_INFO_3,
    };
    const NERR_SUCCESS: u32 = 0;
    const NERR_GROUP_EXISTS: u32 = 2223;
    const ERROR_ALIAS_EXISTS: u32 = 1379;
    const ERROR_MEMBER_IN_ALIAS: u32 = 1378;

    let mut group = wide(&authorized_group());
    let mut comment = wide("Users allowed to run any OpenVPN configuration");
    let info = LOCALGROUP_INFO_1 {
        lgrpi1_name: group.as_mut_ptr(),
        lgrpi1_comment: comment.as_mut_ptr(),
    };
    let mut parm_err = 0u32;
    let r = unsafe {
        NetLocalGroupAdd(
            ptr::null(),
            1,
            &info as *const _ as *const u8,
            &mut parm_err,
        )
    };
    if !matches!(r, NERR_SUCCESS | NERR_GROUP_EXISTS | ERROR_ALIAS_EXISTS) {
        return r as i32;
    }
    let mut name = wide(user);
    let member = LOCALGROUP_MEMBERS_INFO_3 {
        lgrmi3_domainandname: name.as_mut_ptr(),
    };
    let r = unsafe {
        NetLocalGroupAddMembers(
            ptr::null(),
            group.as_ptr(),
            3,
            &member as *const _ as *const u8,
            1,
        )
    };
    match r {
        NERR_SUCCESS | ERROR_MEMBER_IN_ALIAS => 0,
        e => e as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Splits with Windows' own CommandLineToArgvW - the same parser the
    /// service uses to validate the options.
    fn split(line: &str) -> Vec<String> {
        use windows_sys::Win32::Foundation::{LocalFree, HLOCAL};
        use windows_sys::Win32::UI::Shell::CommandLineToArgvW;
        let w = wide(line);
        let mut n = 0i32;
        let argv = unsafe { CommandLineToArgvW(w.as_ptr(), &mut n) };
        let mut out = Vec::new();
        for i in 0..n as usize {
            unsafe {
                let p = *argv.add(i);
                let mut len = 0;
                while *p.add(len) != 0 {
                    len += 1;
                }
                out.push(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)));
            }
        }
        unsafe { LocalFree(argv as HLOCAL) };
        out
    }

    #[test]
    fn arguments_survive_the_windows_parser() {
        let cases = [
            r"C:\Users\Maria Silva\Idlerx\vpn\grupo.ovpn",
            r"C:\nospace\a.ovpn",
            r#"has "quotes" inside"#,
            r"ends with slash\",
            r"\\server\shared folder\x.ovpn",
            "",
        ];
        // first word: CommandLineToArgvW treats argv[0] as the program name
        let line = std::iter::once("openvpn".to_string())
            .chain(cases.iter().map(|c| arg(c)))
            .collect::<Vec<_>>()
            .join(" ");
        let back = split(&line);
        assert_eq!(&back[1..], &cases.map(String::from)[..]);
    }

    #[test]
    fn service_answers() {
        assert_eq!(
            parse_answer("0x00000000\n0x00001a2b\nProcess ID"),
            Ok(0x1a2b)
        );
        let err = parse_answer(
            "0x20000000\nYou have specified an option (--auth-nocache) that may be used only \
             with admin approval.\nGetStartupData",
        );
        let (code, msg) = err.unwrap_err();
        assert_eq!(code, 0x2000_0000);
        assert!(msg.contains("admin approval"));
        assert!(parse_answer("garbage").is_err());
    }
}
