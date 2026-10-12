//! VPN per group: the accounts of a group with a VPN reach the internet only
//! through it, and never through the regular network.
//!
//! - `openvpn`: one OpenVPN connection per group (interactive service, no
//!   administrator rights), without changing the routes of Windows
//! - `proxy`: the group's gate - a local SOCKS5 proxy pinned to the VPN
//!   adapter, closed (kill switch) whenever the tunnel is not up
//! - `dns`: name resolution through the tunnel
//! - `net`: Windows network lookups
//! - `service`, `elevate`, `installer`: OpenVPN's interactive service,
//!   administrator rights on demand, the embedded installer
//! - `secrets`: passwords and token seeds, encrypted by Windows
//! - `totp`: Google Authenticator tokens
//!
//! The group's browser is told to use only the gate (see `panels`), so a
//! page can never open a connection of its own.

mod dns;
mod elevate;
pub mod installer;
mod net;
mod openvpn;
mod proxy;
pub mod secrets;
pub mod service;
pub mod totp;

use crate::config::{self, GroupVpn, VpnAuth};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// VPN files folder (%APPDATA%\Idlerx\vpn): copies of the .ovpn files,
/// encrypted secrets, OpenVPN logs.
pub fn dir() -> PathBuf {
    config::data_dir().join("vpn")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Phase {
    #[default]
    Stopped,
    Connecting,
    Connected,
    /// It was up and dropped: the gate is closed until it comes back.
    Reconnecting,
    /// Will not retry by itself (message for the user).
    Failed(String),
    /// OpenVPN Community is not installed.
    MissingOpenVpn,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub phase: Phase,
    /// Address the sites see (fetched through the tunnel).
    pub ip: Option<Ipv4Addr>,
    /// Bytes per second through the gate, and totals since the app opened.
    pub down_rate: f64,
    pub up_rate: f64,
    pub down_total: u64,
    pub up_total: u64,
    /// Increases every time the tunnel comes up (pages that failed
    /// meanwhile are reloaded).
    pub epoch: u64,
    /// The server has not answered for a while.
    pub slow: bool,
    /// Why OpenVPN last exited by itself.
    pub last_error: Option<String>,
    /// Where the state changes are written (vpn\logs\idlerx-<group>.log).
    events: Option<PathBuf>,
}

impl Status {
    pub fn up(&self) -> bool {
        self.phase == Phase::Connected
    }
}

/// What a connection needs, from the group settings and the secrets.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub group: String,
    pub config: PathBuf,
    pub auth: VpnAuth,
    pub username: String,
    pub secret: secrets::Secret,
}

impl Params {
    pub fn of(group: &str, vpn: &GroupVpn) -> Params {
        Params {
            group: group.to_string(),
            config: PathBuf::from(&vpn.file),
            auth: vpn.auth,
            username: vpn.username.clone(),
            secret: secrets::get(group),
        }
    }
}

struct Tunnel {
    gate: Arc<proxy::Gate>,
    port: u16,
    status: Arc<Mutex<Status>>,
    /// Running connection, with the settings and secrets generation it
    /// was started with.
    connection: Option<(openvpn::Connection, GroupVpn, u64)>,
}

/// Every group's tunnel. Owned by the UI thread.
pub struct Tunnels {
    map: HashMap<String, Tunnel>,
    wake: Wake,
    /// Increases when a password or seed changes (they are not compared
    /// every frame: reading them means decrypting).
    secrets_gen: u64,
}

impl Tunnels {
    pub fn new(wake: Wake) -> Tunnels {
        openvpn::kill_orphans();
        Tunnels {
            map: HashMap::new(),
            wake,
            secrets_gen: 0,
        }
    }

    /// A group's password or token seed changed: reconnect with them.
    pub fn secrets_changed(&mut self) {
        self.secrets_gen += 1;
    }

    /// The group's tunnel, created (with its gate listening) on first use.
    /// The gate's port stays the same while the app runs: the group's
    /// browser is started with it.
    fn tunnel(&mut self, group: &str) -> Option<&mut Tunnel> {
        if !self.map.contains_key(group) {
            let gate = Arc::new(proxy::Gate::default());
            let port = gate.serve().ok()?;
            self.map.insert(
                group.to_string(),
                Tunnel {
                    gate,
                    port,
                    status: Arc::default(),
                    connection: None,
                },
            );
        }
        self.map.get_mut(group)
    }

    /// Port of the group's gate (for the browser arguments).
    pub fn port(&mut self, group: &str) -> Option<u16> {
        self.tunnel(group).map(|t| t.port)
    }

    /// Keeps the group's VPN connected with these settings (connects, or
    /// reconnects when they changed). A failed connection is not retried
    /// until `retry`.
    pub fn ensure(&mut self, group: &str, vpn: &GroupVpn) {
        let wake = self.wake.clone();
        let generation = self.secrets_gen;
        let Some(t) = self.tunnel(group) else {
            return;
        };
        if let Some((c, v, g)) = &t.connection {
            // same settings: running, or finished by itself (a failure that
            // only `retry` starts again)
            if v == vpn && *g == generation {
                return;
            }
            c.stop();
            t.gate.close();
        }
        let params = Params::of(group, vpn);
        let conn = openvpn::start(params, t.gate.clone(), t.status.clone(), wake);
        t.connection = Some((conn, vpn.clone(), generation));
    }

    /// Connects again after a failure (or reconnects at once).
    pub fn retry(&mut self, group: &str, vpn: &GroupVpn) {
        self.stop(group);
        self.ensure(group, vpn);
    }

    /// Disconnects the group (its gate stays closed).
    pub fn stop(&mut self, group: &str) {
        if let Some(t) = self.map.get_mut(group) {
            if let Some((c, _, _)) = &t.connection {
                c.stop();
            }
            t.connection = None;
            t.gate.close();
        }
    }

    /// Something is connecting or connected for the group.
    pub fn running(&self, group: &str) -> bool {
        self.map
            .get(group)
            .and_then(|t| t.connection.as_ref())
            .is_some_and(|(c, _, _)| !c.finished())
    }

    pub fn status(&self, group: &str) -> Status {
        self.map
            .get(group)
            .map(|t| t.status.lock().unwrap_or_else(|e| e.into_inner()).clone())
            .unwrap_or_default()
    }

    /// Groups with a tunnel object (connected or not).
    pub fn groups(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }

    /// Disconnects everything and waits a little for OpenVPN to exit.
    pub fn shutdown(&mut self) {
        for t in self.map.values() {
            if let Some((c, _, _)) = &t.connection {
                c.stop();
            }
            t.gate.close();
        }
        let until = Instant::now() + Duration::from_secs(6);
        while Instant::now() < until
            && self
                .map
                .values()
                .any(|t| t.connection.as_ref().is_some_and(|(c, _, _)| !c.finished()))
        {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Tunnels {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Text of a byte rate ("1.2 MB/s").
pub fn fmt_rate(bytes_per_sec: f64) -> String {
    format!("{}/s", fmt_bytes(bytes_per_sec as u64))
}

pub fn fmt_bytes(b: u64) -> String {
    const K: f64 = 1024.0;
    let b = b as f64;
    if b < K {
        format!("{b:.0} B")
    } else if b < K * K {
        format!("{:.0} KB", b / K)
    } else if b < K * K * K {
        format!("{:.1} MB", b / (K * K))
    } else {
        format!("{:.2} GB", b / (K * K * K))
    }
}

/// The .ovpn uses only files embedded in itself (<ca>...</ca> etc.), so it
/// can be copied into Idlerx's folder and the original moved or deleted.
pub fn self_contained(text: &str) -> bool {
    const FILE_OPTIONS: [&str; 10] = [
        "ca",
        "cert",
        "key",
        "tls-auth",
        "tls-crypt",
        "tls-crypt-v2",
        "pkcs12",
        "secret",
        "dh",
        "crl-verify",
    ];
    !text.lines().any(|l| {
        let mut words = l.split_whitespace();
        let (Some(opt), Some(arg)) = (words.next(), words.next()) else {
            return false;
        };
        FILE_OPTIONS.contains(&opt) && arg != "[inline]"
    })
}

/// The .ovpn makes OpenVPN ask for a username and password.
pub fn asks_password(text: &str) -> bool {
    text.lines()
        .any(|l| l.split_whitespace().next() == Some("auth-user-pass"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ovpn_files() {
        let inline = "client\nremote 1.2.3.4\n<ca>\n-----BEGIN\n</ca>\nkey-direction 1\n";
        assert!(self_contained(inline));
        assert!(!asks_password(inline));
        assert!(!self_contained("client\nca ca.crt\n"));
        assert!(self_contained("client\ntls-auth [inline] 1\n"));
        assert!(asks_password("client\nauth-user-pass\n"));
        assert!(asks_password("auth-user-pass creds.txt"));
        assert!(!asks_password("# auth-user-pass"));
    }

    #[test]
    fn byte_texts() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(2048), "2 KB");
        assert_eq!(fmt_bytes(3 * 1024 * 1024 / 2), "1.5 MB");
        assert_eq!(fmt_rate(1024.0), "1 KB/s");
    }
}
