//! CPU and memory per account.
//!
//! Every account has its own browser process (its own profile folder), so
//! the usage of an account is the sum of that process and all its
//! descendants (renderers, GPU, utilities). Measured in a background thread
//! every 2 seconds.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::{Pid, ProcessesToUpdate, System};

const INTERVAL: Duration = Duration::from_secs(2);

/// Key of the app itself: its process tree contains every account, so it
/// is the grand total.
pub const TOTAL: &str = "";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    /// Percentage of the whole machine (0-100, like Task Manager).
    pub cpu: f32,
    /// Bytes (working set).
    pub memory: u64,
}

#[derive(Clone, Default)]
pub struct Metrics {
    /// account id -> processes whose trees belong to it (its browser, or
    /// its page processes in economy mode)
    watch: Arc<Mutex<HashMap<String, Vec<u32>>>>,
    results: Arc<Mutex<HashMap<String, Usage>>>,
}

impl Metrics {
    /// Starts the measuring thread; `repaint` is called after each round.
    pub fn start(repaint: impl Fn() + Send + 'static) -> Metrics {
        let m = Metrics::default();
        let (watch, results) = (m.watch.clone(), m.results.clone());
        std::thread::spawn(move || {
            let mut sys = System::new();
            let cpus = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1) as f32;
            loop {
                let pids = watch.lock().unwrap().clone();
                if !pids.is_empty() {
                    sys.refresh_processes(ProcessesToUpdate::All, true);
                    let tree = children(&sys);
                    let mut out = HashMap::new();
                    for (id, roots) in pids {
                        let mut u = Usage::default();
                        for pid in roots {
                            let t = sum_tree(&sys, &tree, Pid::from_u32(pid));
                            u.cpu += t.cpu;
                            u.memory += t.memory;
                        }
                        u.cpu = (u.cpu / cpus).min(100.0);
                        out.insert(id, u);
                    }
                    *results.lock().unwrap() = out;
                    repaint();
                }
                std::thread::sleep(INTERVAL);
            }
        });
        m
    }

    pub fn set_watch(&self, pids: HashMap<String, Vec<u32>>) {
        *self.watch.lock().unwrap() = pids;
    }

    pub fn get(&self, id: &str) -> Option<Usage> {
        self.results.lock().unwrap().get(id).copied()
    }
}

fn children(sys: &System) -> HashMap<Pid, Vec<Pid>> {
    let mut map: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for (pid, p) in sys.processes() {
        if let Some(parent) = p.parent() {
            map.entry(parent).or_default().push(*pid);
        }
    }
    map
}

/// Usage of a process plus all its descendants (cpu still per core).
fn sum_tree(sys: &System, tree: &HashMap<Pid, Vec<Pid>>, root: Pid) -> Usage {
    let mut total = Usage::default();
    let mut stack = vec![root];
    let mut seen = std::collections::HashSet::new();
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(p) = sys.process(pid) {
            total.cpu += p.cpu_usage();
            total.memory += p.memory();
        }
        if let Some(kids) = tree.get(&pid) {
            stack.extend(kids.iter().copied());
        }
    }
    total
}

/// "245 MB" / "1.2 GB" (comma in Portuguese).
pub fn fmt_memory(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1024.0 {
        let s = format!("{:.1} GB", mb / 1024.0);
        if crate::i18n::pt() {
            s.replace('.', ",")
        } else {
            s
        }
    } else {
        format!("{mb:.0} MB")
    }
}

/// "2h 14min" / "48min" / "<1min".
pub fn fmt_duration(d: Duration) -> String {
    let min = d.as_secs() / 60;
    match (min / 60, min % 60) {
        (0, 0) => "<1min".into(),
        (0, m) => format!("{m}min"),
        (h, m) => format!("{h}h {m:02}min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatos() {
        assert_eq!(fmt_duration(Duration::from_secs(30)), "<1min");
        assert_eq!(fmt_duration(Duration::from_secs(48 * 60 + 5)), "48min");
        assert_eq!(
            fmt_duration(Duration::from_secs(2 * 3600 + 14 * 60)),
            "2h 14min"
        );
        assert_eq!(fmt_memory(245 * 1024 * 1024), "245 MB");
        assert_eq!(fmt_memory(1288 * 1024 * 1024), "1,3 GB");
    }

    #[test]
    fn mede_o_proprio_processo() {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let tree = children(&sys);
        let u = sum_tree(&sys, &tree, Pid::from_u32(std::process::id()));
        assert!(u.memory > 0);
    }
}
