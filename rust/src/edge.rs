//! Leftovers of the previous engine (WebView2, up to Idlerx 0.10): its
//! folders stay in the profiles folder until the user deletes them in
//! Settings (they can take hundreds of MB).

use std::path::PathBuf;

/// The previous engine's folders (everything in the profiles folder that is
/// not Firefox's).
pub fn old_dirs() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(crate::config::profiles_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_name() != "firefox" && e.path().is_dir())
        .map(|e| e.path())
        .collect()
}

/// Deletes them (asked in Settings), in the background.
pub fn delete_old() {
    std::thread::spawn(|| {
        for dir in old_dirs() {
            for _ in 0..10 {
                if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    });
}
