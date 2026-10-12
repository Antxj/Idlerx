//! Updates through GitHub Releases, with no server of our own.
//!
//! - Unobtrusive check: 30 s after opening and then every hour, the app
//!   queries `releases/latest` on the public GitHub API (which ignores
//!   pre-releases). Failures are silent. Nothing is sent besides the
//!   request itself (GitHub sees the IP and the version in the User-Agent).
//! - "Update now": downloads the release Idlerx.exe, verifies and replaces the
//!   executable and reopens the app (the accounts that were open are
//!   reopened from the saved settings).
//!
//! Replacement safety:
//! - the downloaded file must have exactly the SHA-256 that GitHub publishes
//!   for it (the asset `digest` field);
//! - if the current executable is signed (Authenticode), the new one must
//!   have a valid signature FROM THE SAME publisher. From the first signed
//!   version on, an unsigned version is never installed.
//!
//! Everything through Windows' own APIs (WinHTTP, BCrypt, WinVerifyTrust):
//! no new dependency, and WinHTTP honors the system proxy.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const REPOSITORY: &str = "Antxj/Idlerx";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Name of the release asset that contains the app.
const ASSET: &str = "Idlerx.exe";
/// Right after the app opens (a moment for the window to show up).
const FIRST_CHECK: Duration = Duration::from_secs(3);
const INTERVAL: Duration = Duration::from_secs(60 * 60);
/// How often the clock is looked at (the wait follows the real clock, so a
/// computer that slept does not delay the next check).
const TICK: Duration = Duration::from_secs(60);
const JSON_LIMIT: usize = 2 * 1024 * 1024;
const EXE_LIMIT: usize = 64 * 1024 * 1024;

/// A published version, newer than the current one.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub number: String,
    /// Release page (what's new).
    pub page: String,
    /// Idlerx.exe link; empty if the release does not have the asset.
    pub download: String,
    pub size: u64,
    /// SHA-256 published by GitHub (lowercase hex).
    pub sha256: Option<String>,
}

impl Version {
    /// Can be installed by the app (otherwise, only from the page).
    pub fn installable(&self) -> bool {
        !self.download.is_empty() && self.sha256.is_some()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    /// Check requested by the user, in progress.
    Checking,
    /// Result of the check requested by the user.
    UpToDate,
    CheckFailed(String),
    Available(Version),
    /// Downloading (fraction from 0 to 1).
    Downloading(Version, f32),
    InstallFailed(Version, String),
    /// Installed: the app is closing to open the new version.
    Restarting(Version),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);
static ON_CHANGE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Function called on every state change (to repaint the UI).
pub fn on_change(f: impl Fn() + Send + Sync + 'static) {
    let _ = ON_CHANGE.set(Box::new(f));
}

pub fn state() -> State {
    STATE.lock().unwrap().clone()
}

fn set_state(e: State) {
    *STATE.lock().unwrap() = e;
    if let Some(f) = ON_CHANGE.get() {
        f();
    }
}

/// Number of the new version, while there is one to offer.
pub fn available() -> Option<String> {
    match state() {
        State::Available(v)
        | State::Downloading(v, _)
        | State::InstallFailed(v, _)
        | State::Restarting(v) => Some(v.number),
        _ => None,
    }
}

/// Hides the result of the manual check (when leaving the screen).
pub fn clear_result() {
    if matches!(state(), State::UpToDate | State::CheckFailed(_)) {
        set_state(State::Idle);
    }
}

pub fn repository_page() -> String {
    format!("https://github.com/{REPOSITORY}")
}

pub fn releases_page() -> String {
    format!("https://github.com/{REPOSITORY}/releases")
}

/// IDLERX_UPDATE_URL changes the queried address (tests only).
fn api_url() -> String {
    std::env::var("IDLERX_UPDATE_URL")
        .unwrap_or_else(|_| format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"))
}

/// Automatic check, while `enabled()` says yes.
pub fn start_periodic_check(enabled: fn() -> bool) {
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        let mut last_check: Option<std::time::SystemTime> = None;
        loop {
            let due = last_check
                .and_then(|t| t.elapsed().ok())
                .is_none_or(|elapsed| elapsed >= INTERVAL);
            // once a new version was found, nothing more to ask until a restart
            if due && enabled() && available().is_none() {
                check(false);
                last_check = Some(std::time::SystemTime::now());
            }
            std::thread::sleep(TICK);
        }
    });
}

/// Check requested by the user: shows the result, failures included.
pub fn check_now() {
    std::thread::spawn(|| check(true));
}

fn check(manual: bool) {
    if matches!(
        state(),
        State::Checking | State::Downloading(..) | State::Restarting(_)
    ) {
        return;
    }
    if manual {
        set_state(State::Checking);
    }
    match query(&api_url(), CURRENT_VERSION) {
        Ok(Some(v)) => set_state(State::Available(v)),
        Ok(None) if manual => set_state(State::UpToDate),
        Err(e) if manual => set_state(State::CheckFailed(e)),
        _ => {}
    }
}

fn query(url: &str, current: &str) -> Result<Option<Version>, String> {
    let body = http_get(url, JSON_LIMIT, &mut |_| {}).map_err(|e| match e {
        // private/non-existent repository or no published release
        HttpError::Status(404) => tr!(
            "Nenhuma versão publicada foi encontrada.",
            "No published version was found."
        )
        .to_string(),
        outro => outro.message(),
    })?;
    parse_release(&body, current)
}

/// Reads the `releases/latest` response; Some if newer than `current`.
fn parse_release(body: &[u8], current: &str) -> Result<Option<Version>, String> {
    let unexpected = || {
        tr!(
            "Resposta inesperada do GitHub.",
            "Unexpected response from GitHub."
        )
        .to_string()
    };
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|_| unexpected())?;
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) {
        return Ok(None);
    }
    let tag = v["tag_name"].as_str().ok_or_else(unexpected)?;
    let number = tag.trim_start_matches(['v', 'V']).to_string();
    if !is_newer(&number, current) {
        return Ok(None);
    }
    let page = v["html_url"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(releases_page);
    let asset = v["assets"].as_array().and_then(|list| {
        list.iter().find(|a| {
            a["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(ASSET))
        })
    });
    let mut version = Version {
        number,
        page,
        download: String::new(),
        size: 0,
        sha256: None,
    };
    if let Some(a) = asset {
        version.download = a["browser_download_url"]
            .as_str()
            .unwrap_or_default()
            .into();
        version.size = a["size"].as_u64().unwrap_or(0);
        version.sha256 = a["digest"]
            .as_str()
            .and_then(|d| d.strip_prefix("sha256:"))
            .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
            .map(|h| h.to_ascii_lowercase());
    }
    Ok(Some(version))
}

/// "1.2.3" > "1.2.2"? Versions with a suffix ("1.3.0-rc1") are never
/// offered: pre-releases are left to those who download from the page.
fn is_newer(new: &str, current: &str) -> bool {
    fn parts(s: &str) -> Option<[u64; 3]> {
        let mut out = [0u64; 3];
        let mut n = 0;
        for p in s.trim().split('.') {
            if n == 3 {
                return None;
            }
            out[n] = p.parse().ok()?;
            n += 1;
        }
        (n > 0).then_some(out)
    }
    match (
        parts(new),
        parts(current.split(['-', '+']).next().unwrap_or(current)),
    ) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

// ------------------------------------------------------------- install --

/// Downloads, verifies and installs the version in the background; then reopens the app.
pub fn install(v: Version) {
    if matches!(state(), State::Downloading(..) | State::Restarting(_)) {
        return;
    }
    set_state(State::Downloading(v.clone(), 0.0));
    std::thread::spawn(move || {
        let target = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                set_state(State::InstallFailed(
                    v,
                    trf!(
                        "Não encontrei o executável atual: {e}",
                        "Could not find the current executable: {e}"
                    ),
                ));
                return;
            }
        };
        let mut last = 0u32;
        let result = download_and_replace(&v, &target, &mut |fraction| {
            // repaint on every 1% (not on every downloaded chunk)
            let pct = (fraction * 100.0) as u32;
            if pct != last {
                last = pct;
                set_state(State::Downloading(v.clone(), fraction));
            }
        });
        match result {
            Ok(()) => {
                // the new process is started first: Restarting closes this one
                match reopen(&target) {
                    Ok(()) => set_state(State::Restarting(v)),
                    Err(e) => set_state(State::InstallFailed(
                        v,
                        trf!(
                            "A versão nova foi instalada, mas não consegui reabrir o \
                             aplicativo ({e}). Feche e abra de novo.",
                            "The new version was installed, but the app could not be \
                             reopened ({e}). Close it and open it again."
                        ),
                    )),
                }
            }
            Err(e) => set_state(State::InstallFailed(v, e)),
        }
    });
}

/// `Idlerx.exe` -> `Idlerx.exe.<suffix>`, in the same folder (same disk: the swap
/// is a simple rename).
fn sibling(target: &Path, suffix: &str) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| ASSET.into());
    target.with_file_name(format!("{name}.{suffix}"))
}

fn download_and_replace(
    v: &Version,
    target: &Path,
    progress: &mut dyn FnMut(f32),
) -> Result<(), String> {
    if v.download.is_empty() {
        return Err(trf!(
            "A versão {} não tem o {ASSET} anexado.",
            "Version {} does not have {ASSET} attached.",
            v.number
        ));
    }
    let expected = v.sha256.as_deref().ok_or_else(|| {
        tr!(
            "O GitHub não informou o SHA-256 deste arquivo, então ele não pode ser \
             conferido. Baixe pela página da versão.",
            "GitHub did not provide the SHA-256 of this file, so it cannot be \
             verified. Download it from the release page."
        )
        .to_string()
    })?;
    let total = v.size;
    let data = http_get(&v.download, EXE_LIMIT, &mut |read| {
        if total > 0 {
            progress((read as f32 / total as f32).min(1.0));
        }
    })
    .map_err(|e| trf!("Falha no download: {}", "Download failed: {}", e.message()))?;

    if total > 0 && data.len() as u64 != total {
        return Err(tr!(
            "O download veio incompleto. Nada foi alterado.",
            "The download is incomplete. Nothing was changed."
        )
        .into());
    }
    if sha256_hex(&data).as_deref() != Some(expected) {
        return Err(tr!(
            "O arquivo baixado não confere com o publicado (SHA-256 diferente). \
             Nada foi alterado.",
            "The downloaded file does not match the published one (different \
             SHA-256). Nothing was changed."
        )
        .into());
    }
    if !data.starts_with(b"MZ") {
        return Err(tr!(
            "O arquivo baixado não é um executável. Nada foi alterado.",
            "The downloaded file is not an executable. Nothing was changed."
        )
        .into());
    }

    let new_file = sibling(target, "novo");
    let old_file = sibling(target, "antigo");
    std::fs::write(&new_file, &data).map_err(|e| {
        trf!(
            "Não consegui gravar a versão nova na pasta do aplicativo: {e}",
            "Could not write the new version to the app folder: {e}"
        )
    })?;

    // from the first signed version on, only the same publisher is accepted
    if let Some(publisher) = signer(target) {
        if signer(&new_file).as_deref() != Some(publisher.as_str()) {
            let _ = std::fs::remove_file(&new_file);
            return Err(trf!(
                "O arquivo baixado não tem a assinatura digital de \"{publisher}\". \
                 Nada foi alterado.",
                "The downloaded file is not digitally signed by \"{publisher}\". \
                 Nothing was changed."
            ));
        }
    }

    // Windows allows renaming the running executable (not deleting it): the
    // current one becomes .old_file, deleted on the next start
    let _ = std::fs::remove_file(&old_file);
    let fail = |e: std::io::Error| {
        trf!(
            "Não consegui substituir o aplicativo: {e}",
            "Could not replace the app: {e}"
        )
    };
    if let Err(e) = std::fs::rename(target, &old_file) {
        let _ = std::fs::remove_file(&new_file);
        return Err(fail(e));
    }
    if let Err(e) = std::fs::rename(&new_file, target) {
        let _ = std::fs::rename(&old_file, target);
        let _ = std::fs::remove_file(&new_file);
        return Err(fail(e));
    }
    Ok(())
}

/// Opens the new version (which waits for this process to end). The
/// interface sees `State::Restarting`, saves the settings and closes.
fn reopen(target: &Path) -> std::io::Result<()> {
    std::process::Command::new(target)
        .arg(ARG_AFTER_UPDATE)
        .arg(std::process::id().to_string())
        .spawn()
        .map(|_| ())
}

pub const ARG_AFTER_UPDATE: &str = "--after-update";

/// On the first start after an update: waits for the previous version to close
/// (it is still saving the settings and holding the single instance).
pub fn wait_for_process(pid: u32, timeout: Duration) {
    const SYNCHRONIZE: u32 = 0x0010_0000;
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(acesso: u32, herdar: i32, pid: u32) -> *mut c_void;
        fn WaitForSingleObject(h: *mut c_void, ms: u32) -> u32;
        fn CloseHandle(h: *mut c_void) -> i32;
    }
    unsafe {
        let h = OpenProcess(SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, timeout.as_millis() as u32);
            CloseHandle(h);
        }
    }
}

/// Deletes leftovers of an update (the previous executable).
pub fn clean_leftovers() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(sibling(&exe, "antigo"));
        let _ = std::fs::remove_file(sibling(&exe, "novo"));
    }
}

// ------------------------------------------------------- SHA-256 (BCrypt) --

fn sha256_hex(data: &[u8]) -> Option<String> {
    use windows_sys::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut out = [0u8; 32];
    let status = unsafe {
        BCryptHash(
            BCRYPT_SHA256_ALG_HANDLE,
            ptr::null(),
            0,
            data.as_ptr(),
            u32::try_from(data.len()).ok()?,
            out.as_mut_ptr(),
            out.len() as u32,
        )
    };
    (status == 0).then(|| out.iter().map(|b| format!("{b:02x}")).collect())
}

// ----------------------------------------------- signature (WinVerifyTrust) --

/// Publisher name, if the file has a valid Authenticode signature.
pub fn signer(path: &Path) -> Option<String> {
    use windows_sys::Win32::Security::Cryptography::{
        CertGetNameStringW, CERT_NAME_SIMPLE_DISPLAY_TYPE,
    };
    use windows_sys::Win32::Security::WinTrust::*;

    let path_w = crate::single::wide(&path.to_string_lossy());
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path_w.as_ptr(),
        hFile: ptr::null_mut(),
        pgKnownSubject: ptr::null_mut(),
    };
    let mut data: WINTRUST_DATA = unsafe { std::mem::zeroed() };
    data.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as u32;
    data.dwUIChoice = WTD_UI_NONE;
    data.fdwRevocationChecks = WTD_REVOKE_WHOLECHAIN;
    data.dwProvFlags = WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT;
    data.dwUnionChoice = WTD_CHOICE_FILE;
    data.Anonymous.pFile = &mut file_info;
    data.dwStateAction = WTD_STATEACTION_VERIFY;

    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let no_window = -1isize as *mut c_void; // INVALID_HANDLE_VALUE: never shows UI
    let result =
        unsafe { WinVerifyTrust(no_window, &mut action, &mut data as *mut _ as *mut c_void) };

    let mut name = None;
    if result == 0 {
        unsafe {
            let prov = WTHelperProvDataFromStateData(data.hWVTStateData);
            let sgnr = if prov.is_null() {
                ptr::null_mut()
            } else {
                WTHelperGetProvSignerFromChain(prov, 0, 0, 0)
            };
            let cert = if sgnr.is_null() {
                ptr::null_mut()
            } else {
                WTHelperGetProvCertFromChain(sgnr, 0)
            };
            if !cert.is_null() && !(*cert).pCert.is_null() {
                let mut buf = [0u16; 256];
                let n = CertGetNameStringW(
                    (*cert).pCert,
                    CERT_NAME_SIMPLE_DISPLAY_TYPE,
                    0,
                    ptr::null(),
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                );
                if n > 1 {
                    name = Some(String::from_utf16_lossy(&buf[..n as usize - 1]));
                }
            }
        }
    }
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(no_window, &mut action, &mut data as *mut _ as *mut c_void);
    }
    name.filter(|n| !n.is_empty())
}

// ------------------------------------------------------------ HTTP (WinHTTP) --

#[derive(Debug, PartialEq)]
enum HttpError {
    /// Network/TLS failure (WinHTTP code).
    Network(u32),
    Status(u32),
    TooLarge,
    Url,
}

impl HttpError {
    fn message(&self) -> String {
        match self {
            HttpError::Network(_) => tr!(
                "Sem conexão com o GitHub. Verifique a internet.",
                "Could not reach GitHub. Check the internet connection."
            )
            .into(),
            HttpError::Status(s) => {
                trf!(
                    "O GitHub respondeu com erro (HTTP {s}).",
                    "GitHub returned an error (HTTP {s})."
                )
            }
            HttpError::TooLarge => tr!("Resposta grande demais.", "Response too large.").into(),
            HttpError::Url => tr!("Endereço inválido.", "Invalid address.").into(),
        }
    }
}

/// Closes the WinHTTP handle when it goes out of scope.
struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { windows_sys::Win32::Networking::WinHttp::WinHttpCloseHandle(self.0) };
        }
    }
}

fn last_error() -> u32 {
    unsafe { windows_sys::Win32::Foundation::GetLastError() }
}

/// Simple GET (follows HTTPS redirects, like the one of the asset
/// downloads). `progress` receives the bytes read so far.
fn http_get(
    url: &str,
    timeout: usize,
    progress: &mut dyn FnMut(u64),
) -> Result<Vec<u8>, HttpError> {
    use windows_sys::Win32::Networking::WinHttp::*;

    let url_w: Vec<u16> = url.encode_utf16().collect();
    let mut parts: URL_COMPONENTS = unsafe { std::mem::zeroed() };
    parts.dwStructSize = std::mem::size_of::<URL_COMPONENTS>() as u32;
    // -1: returns pointers into the URL itself
    parts.dwSchemeLength = u32::MAX;
    parts.dwHostNameLength = u32::MAX;
    parts.dwUrlPathLength = u32::MAX;
    parts.dwExtraInfoLength = u32::MAX;
    if unsafe { WinHttpCrackUrl(url_w.as_ptr(), url_w.len() as u32, 0, &mut parts) } == 0 {
        return Err(HttpError::Url);
    }
    let slice = |p: *const u16, n: u32| -> Vec<u16> {
        if p.is_null() {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(p, n as usize) }.to_vec()
        }
    };
    let mut host = slice(parts.lpszHostName, parts.dwHostNameLength);
    host.push(0);
    let mut path = slice(parts.lpszUrlPath, parts.dwUrlPathLength);
    path.extend(slice(parts.lpszExtraInfo, parts.dwExtraInfoLength));
    if path.is_empty() {
        path.push(b'/' as u16);
    }
    path.push(0);
    let https = parts.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS;

    let agent = crate::single::wide(&format!(
        "Idlerx/{CURRENT_VERSION} (+https://github.com/{REPOSITORY})"
    ));
    let session = Handle(unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err(HttpError::Network(last_error()));
    }
    unsafe { WinHttpSetTimeouts(session.0, 15_000, 15_000, 30_000, 30_000) };

    let connection = Handle(unsafe { WinHttpConnect(session.0, host.as_ptr(), parts.nPort, 0) });
    if connection.0.is_null() {
        return Err(HttpError::Network(last_error()));
    }
    let verb = crate::single::wide("GET");
    let request = Handle(unsafe {
        WinHttpOpenRequest(
            connection.0,
            verb.as_ptr(),
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            if https { WINHTTP_FLAG_SECURE } else { 0 },
        )
    });
    if request.0.is_null() {
        return Err(HttpError::Network(last_error()));
    }
    let headers = crate::single::wide(
        "Accept: application/vnd.github+json, application/octet-stream\r\n\
         X-GitHub-Api-Version: 2022-11-28\r\n",
    );
    let ok = unsafe {
        WinHttpSendRequest(request.0, headers.as_ptr(), u32::MAX, ptr::null(), 0, 0, 0) != 0
            && WinHttpReceiveResponse(request.0, ptr::null_mut()) != 0
    };
    if !ok {
        return Err(HttpError::Network(last_error()));
    }

    let mut status: u32 = 0;
    let mut len = std::mem::size_of::<u32>() as u32;
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut status as *mut u32 as *mut c_void,
            &mut len,
            ptr::null_mut(),
        )
    };
    if status != 200 {
        return Err(HttpError::Status(status));
    }

    let mut body = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let mut read: u32 = 0;
        let ok = unsafe {
            WinHttpReadData(
                request.0,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
                &mut read,
            )
        };
        if ok == 0 {
            return Err(HttpError::Network(last_error()));
        }
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buf[..read as usize]);
        if body.len() > timeout {
            return Err(HttpError::TooLarge);
        }
        progress(body.len() as u64);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn compares_versions() {
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(is_newer("1.1", "1.0.9"));
        assert!(is_newer("2.0.0", "1.12.3"));
        assert!(is_newer("1.0.10", "1.0.9")); // numeric, not alphabetical
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.9", "1.0.0"));
        // pre-releases and garbage are never offered
        assert!(!is_newer("1.1.0-rc1", "1.0.0"));
        assert!(!is_newer("abc", "1.0.0"));
        assert!(!is_newer("", "1.0.0"));
        assert!(!is_newer("1.2.3.4", "1.0.0"));
    }

    fn release(tag: &str, anexos: &str) -> Vec<u8> {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://github.com/x/y/releases/tag/{tag}",
               "draft":false,"prerelease":false,"assets":[{anexos}]}}"#
        )
        .into_bytes()
    }

    const HASH: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn reads_the_github_release() {
        let asset = format!(
            r#"{{"name":"LEIA-ME.txt","browser_download_url":"https://x/leia","size":10}},
               {{"name":"Idlerx.exe","browser_download_url":"https://x/Idlerx.exe","size":123,
                 "digest":"sha256:{}"}}"#,
            HASH.to_uppercase()
        );
        let v = parse_release(&release("v1.0.1", &asset), "1.0.0")
            .unwrap()
            .unwrap();
        assert_eq!(v.number, "1.0.1");
        assert_eq!(v.download, "https://x/Idlerx.exe");
        assert_eq!(v.size, 123);
        assert_eq!(v.sha256.as_deref(), Some(HASH));
        assert!(v.installable());
        assert!(v.page.ends_with("/v1.0.1"));

        // same or older version: nothing to offer
        assert_eq!(
            parse_release(&release("v1.0.0", &asset), "1.0.0").unwrap(),
            None
        );
        // without the published digest, only from the page
        let no_hash =
            r#"{"name":"Idlerx.exe","browser_download_url":"https://x/Idlerx.exe","size":1}"#;
        let v = parse_release(&release("v2.0.0", no_hash), "1.0.0")
            .unwrap()
            .unwrap();
        assert!(!v.installable());
        // without the asset, only from the page
        let v = parse_release(&release("v2.0.0", ""), "1.0.0")
            .unwrap()
            .unwrap();
        assert!(!v.installable());
        // a pre-release is never offered
        let pre = String::from_utf8(release("v9.0.0", &asset))
            .unwrap()
            .replace(r#""prerelease":false"#, r#""prerelease":true"#);
        assert_eq!(parse_release(pre.as_bytes(), "1.0.0").unwrap(), None);
        assert!(parse_release(b"<html>", "1.0.0").is_err());
    }

    #[test]
    fn sha256_through_windows() {
        assert_eq!(sha256_hex(b"abc").as_deref(), Some(HASH));
    }

    /// Minimal HTTP server on 127.0.0.1: answers each known path.
    fn server(routes: Vec<(&'static str, Vec<u8>)>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut s = s;
                let mut req = [0u8; 4096];
                let n = s.read(&mut req).unwrap_or(0);
                let line = String::from_utf8_lossy(&req[..n]);
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let resp = match routes.iter().find(|(c, _)| *c == path) {
                    Some((_, body)) => {
                        let mut r = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        r.extend_from_slice(body);
                        r
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = s.write_all(&resp);
            }
        });
        base
    }

    #[test]
    fn downloads_checks_and_replaces_the_executable() {
        let new_exe = b"MZ versao nova do aplicativo".to_vec();
        let hash = sha256_hex(&new_exe).unwrap();
        let base = server(vec![("/Idlerx.exe", new_exe.clone())]);

        let dir =
            std::env::temp_dir().join(format!("idlerx-teste-atualizacao-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("Idlerx.exe");

        let version = |sha: &str| Version {
            number: "9.9.9".into(),
            page: String::new(),
            download: format!("{base}/Idlerx.exe"),
            size: new_exe.len() as u64,
            sha256: Some(sha.into()),
        };

        // hash different from the published one: refuses and touches nothing
        std::fs::write(&target, b"MZ versao atual").unwrap();
        let wrong = "0".repeat(64);
        let e = download_and_replace(&version(&wrong), &target, &mut |_| {}).unwrap_err();
        assert!(e.contains("SHA-256"), "{e}");
        assert_eq!(std::fs::read(&target).unwrap(), b"MZ versao atual");
        assert!(!sibling(&target, "novo").exists());

        // right hash: swaps, keeps the previous one as .old_file and reports progress
        let mut fractions = Vec::new();
        download_and_replace(&version(&hash), &target, &mut |f| fractions.push(f)).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), new_exe);
        assert_eq!(
            std::fs::read(sibling(&target, "antigo")).unwrap(),
            b"MZ versao atual"
        );
        assert!(!sibling(&target, "novo").exists());
        assert_eq!(fractions.last().copied(), Some(1.0));

        // asset missing on the server
        let mut missing = version(&hash);
        missing.download = format!("{base}/nao-existe");
        assert!(download_and_replace(&missing, &target, &mut |_| {}).is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn queries_the_api_over_http() {
        let body = release("v9.9.9", "");
        let base = server(vec![("/latest", body)]);
        let v = query(&format!("{base}/latest"), "1.0.0").unwrap().unwrap();
        assert_eq!(v.number, "9.9.9");
        let e = query(&format!("{base}/outro"), "1.0.0").unwrap_err();
        assert!(e.contains("Nenhuma versão publicada"), "{e}");
    }

    #[test]
    fn digital_signature() {
        // unsigned file
        let p = std::env::temp_dir().join(format!(
            "idlerx-teste-assinatura-{}.exe",
            std::process::id()
        ));
        std::fs::write(&p, b"MZ nada assinado").unwrap();
        assert_eq!(signer(&p), None);
        let _ = std::fs::remove_file(&p);
    }
}
