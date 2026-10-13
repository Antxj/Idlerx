//! Log of what happens to the pages (`logs\pages.log` in the data folder):
//! loads and reloads with their reason, crash recoveries, accounts opened
//! again, pages hidden and shown. Lets a strange moment in a game (a
//! character that left its hunt, say) be matched to what Idlerx did then.
//! Kept small: past 1 MB it moves to pages.old.log and starts over.

use std::io::Write;
use std::path::PathBuf;

const LIMIT: u64 = 1024 * 1024;

fn path() -> PathBuf {
    crate::config::data_dir().join("logs").join("pages.log")
}

fn timestamp() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}

/// One line: time, account and what happened.
pub fn log(account: &str, what: &str) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LIMIT) {
        let _ = std::fs::rename(&path, path.with_file_name("pages.old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{}  [{account}]  {what}", timestamp());
    }
}
