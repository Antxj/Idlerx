//! One OpenVPN connection per group, in a thread of its own.
//!
//! OpenVPN is started through the interactive service (no administrator
//! rights) with options that keep the rest of Windows out of the tunnel:
//! the routes and DNS servers pushed by the server are ignored, and the only
//! route added is a default route through the VPN with the worst possible
//! metric - the regular network keeps winning for everything else, while
//! the group's gate (pinned to the VPN adapter) has a way out.
//!
//! The thread follows OpenVPN through its management interface and drives
//! the gate: it opens only after the tunnel proved to carry traffic (the
//! public address was fetched through it), and closes at once on any other
//! state, or when the adapter disappears.

use super::proxy::{self, Gate, Route};
use super::service::{self, Failure};
use super::{net, totp, Params, Phase, Status, Wake};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Wait between attempts after OpenVPN exits by itself.
const RETRY: Duration = Duration::from_secs(5);
/// Time without an answer from the server before the "not responding" notice.
const NO_ANSWER: Duration = Duration::from_secs(30);
const MAX_TOKEN_FAILURES: u32 = 3;
/// Default route through the VPN with the worst metric: only sockets pinned
/// to the VPN adapter use it.
const LOW_PRIORITY_ROUTE: &str = "--route 0.0.0.0 0.0.0.0 vpn_gateway 9999";

/// Handle of a running connection.
pub struct Connection {
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}

impl Connection {
    /// Asks the connection to end (OpenVPN is told to exit).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn finished(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
}

/// Why an attempt ended.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// Stop requested.
    Stopped,
    /// OpenVPN exited by itself; try again in a moment.
    Exited,
    /// The fast adapter (DCO) is held by another OpenVPN: use the normal one.
    NoDco,
    /// No free network adapter of this type (tapctl hwid): create one.
    NeedAdapter(&'static str),
    /// Retrying would not help (message for the user).
    Fatal(String),
    MissingOpenVpn,
}

/// Shared bookkeeping of the openvpn.exe processes started by Idlerx, saved
/// to disk so the next run can clean up after a crash.
static PROCESSES: Mutex<Vec<(String, u32)>> = Mutex::new(Vec::new());

fn pids_file() -> PathBuf {
    super::dir().join("openvpn.pids")
}

fn register_process(group: &str, pid: Option<u32>) {
    let mut list = PROCESSES.lock().unwrap_or_else(|e| e.into_inner());
    list.retain(|(g, _)| g != group);
    if let Some(pid) = pid {
        list.push((group.to_string(), pid));
    }
    let text: String = list.iter().map(|(_, pid)| format!("{pid}\n")).collect();
    let _ = std::fs::create_dir_all(super::dir());
    let _ = std::fs::write(pids_file(), text);
}

/// At startup: ends openvpn.exe processes left behind by a previous run that
/// did not end normally.
pub fn kill_orphans() {
    let text = std::fs::read_to_string(pids_file()).unwrap_or_default();
    for pid in text.lines().filter_map(|l| l.trim().parse::<u32>().ok()) {
        service::kill_if_openvpn(pid);
    }
    let _ = std::fs::remove_file(pids_file());
}

pub fn log_path(group: &str) -> PathBuf {
    super::dir()
        .join("logs")
        .join(format!("openvpn-{group}.log"))
}

/// Changes the shared status and wakes the interface. State changes also go
/// to the group's events file, to explain what happened later.
fn update(status: &Mutex<Status>, wake: &Wake, f: impl FnOnce(&mut Status)) {
    let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
    let before = s.phase.clone();
    f(&mut s);
    if s.phase != before {
        if let Some(path) = &s.events {
            note(path, &format!("{:?} (IP {:?})", s.phase, s.ip));
        }
    }
    drop(s);
    wake();
}

/// Appends a line to an events file (kept small: it starts over past 256 KB).
fn note(path: &Path, text: &str) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let big = std::fs::metadata(path).is_ok_and(|m| m.len() > 256 * 1024);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(!big)
        .write(true)
        .truncate(big)
        .open(path);
    if let Ok(mut f) = file {
        let _ = writeln!(f, "{secs} {text}");
    }
}

pub fn events_path(group: &str) -> PathBuf {
    super::dir()
        .join("logs")
        .join(format!("idlerx-{group}.log"))
}

/// Starts the group's connection in the background.
pub fn start(p: Params, gate: Arc<Gate>, status: Arc<Mutex<Status>>, wake: Wake) -> Connection {
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let (stop2, done2) = (stop.clone(), done.clone());
    let _ = std::fs::create_dir_all(super::dir().join("logs"));
    update(&status, &wake, |s| {
        s.events = Some(events_path(&p.group));
        s.phase = Phase::Connecting;
        s.slow = false;
    });
    let spawned = std::thread::Builder::new()
        .name("vpn-connection".into())
        .spawn(move || {
            run(&p, &gate, &status, &stop2, &wake);
            gate.close();
            update(&status, &wake, |s| {
                if !matches!(s.phase, Phase::Failed(_) | Phase::MissingOpenVpn) {
                    s.phase = Phase::Stopped;
                }
                s.down_rate = 0.0;
                s.up_rate = 0.0;
            });
            done2.store(true, Ordering::SeqCst);
        });
    if spawned.is_err() {
        done.store(true, Ordering::SeqCst);
    }
    Connection { stop, done }
}

/// Sleeps up to `d`; false if a stop was requested meanwhile.
fn pause(d: Duration, stop: &AtomicBool) -> bool {
    let until = Instant::now() + d;
    while Instant::now() < until {
        if stop.load(Ordering::SeqCst) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    !stop.load(Ordering::SeqCst)
}

fn run(p: &Params, gate: &Arc<Gate>, status: &Mutex<Status>, stop: &AtomicBool, wake: &Wake) {
    let mut no_dco = false;
    let mut adapter_created = false;
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match attempt(p, gate, status, stop, no_dco, wake) {
            Outcome::Stopped => return,
            Outcome::NoDco => no_dco = true,
            Outcome::NeedAdapter(hwid) if !adapter_created => {
                adapter_created = true;
                if let Err(e) = create_adapter(hwid) {
                    let msg = trf!(
                        "Não há adaptador de rede livre para mais uma VPN ao mesmo tempo, e não \
                         foi possível criar outro: {e}",
                        "There is no free network adapter for one more VPN at the same time, \
                         and another one could not be created: {e}"
                    );
                    update(status, wake, |s| s.phase = Phase::Failed(msg));
                    return;
                }
            }
            Outcome::NeedAdapter(_) => {
                let msg = tr!(
                    "Não há adaptador de rede livre para mais uma VPN ao mesmo tempo.",
                    "There is no free network adapter for one more VPN at the same time."
                );
                update(status, wake, |s| s.phase = Phase::Failed(msg.into()));
                return;
            }
            Outcome::Fatal(msg) => {
                update(status, wake, |s| s.phase = Phase::Failed(msg));
                return;
            }
            Outcome::MissingOpenVpn => {
                update(status, wake, |s| s.phase = Phase::MissingOpenVpn);
                return;
            }
            Outcome::Exited => {
                update(status, wake, |s| s.phase = Phase::Reconnecting);
                if !pause(RETRY, stop) {
                    return;
                }
            }
        }
    }
}

/// Creates another virtual adapter with OpenVPN's own tapctl.exe. Needs
/// administrator rights: Windows asks for them (UAC) at this moment.
fn create_adapter(hwid: &str) -> Result<(), String> {
    let tapctl = super::installer::openvpn_dir()
        .ok_or_else(|| tr!("OpenVPN não encontrado", "OpenVPN not found").to_string())?
        .join("bin")
        .join("tapctl.exe");
    match super::elevate::run_as_admin(&tapctl, &format!("create --hwid {hwid}"))? {
        0 => Ok(()),
        code => Err(trf!(
            "o tapctl falhou (código {code})",
            "tapctl failed (code {code})"
        )),
    }
}

/// When the OpenVPN log says there is no free network adapter, the type to
/// create (tapctl hwid). Each simultaneous connection needs its own.
fn missing_adapter(log: &str) -> Option<&'static str> {
    let line = log
        .lines()
        .find(|l| l.contains("currently in use or disabled") || l.contains("create an adapter"))?;
    let l = line.to_lowercase();
    Some(if l.contains("ovpn-dco") {
        "ovpn-dco"
    } else if l.contains("wintun") {
        "wintun"
    } else {
        "root\\tap0901"
    })
}

/// Last error lines of the OpenVPN log, to explain a failure.
fn log_errors(log: &str) -> String {
    let errors: Vec<&str> = log
        .lines()
        .filter(|l| {
            let l = l.to_lowercase();
            l.contains("error") || l.contains("fatal") || l.contains("exiting due to")
        })
        .map(str::trim)
        .collect();
    errors[errors.len().saturating_sub(3)..].join("\n")
}

/// DNS servers the VPN pushed (ignored by Windows, used by the gate through
/// the tunnel), read from the OpenVPN log.
fn pushed_dns(log: &str) -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    for line in log.lines().filter(|l| l.contains("PUSH_REPLY")) {
        for part in line.split(',') {
            let part = part.trim();
            let v = part
                .strip_prefix("dhcp-option DNS ")
                .or_else(|| part.strip_prefix("dns server 0 address "));
            if let Some(ip) = v.and_then(|s| s.trim().parse().ok()) {
                if !out.contains(&ip) {
                    out.push(ip);
                }
            }
        }
    }
    out
}

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(7505)
}

fn random_password() -> String {
    let mut b = [0u8; 18];
    let _ = getrandom::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Waits until the token window has enough time left; None if a stop was
/// requested meanwhile.
fn fresh_token(seed: &str, force_next: bool, stop: &AtomicBool) -> Option<String> {
    let remaining = totp::seconds_remaining();
    let wait = force_next || remaining < totp::MIN_TOKEN_LIFETIME;
    if wait && !pause(Duration::from_millis(remaining * 1000 + 500), stop) {
        return None;
    }
    totp::totp_now(seed)
}

/// Explains the one-time authorization and asks whether to go ahead.
fn ask_authorization() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONINFORMATION, MB_SETFOREGROUND, MB_YESNO,
    };
    let group = service::authorized_group();
    let text = super::elevate::wide(&trf!(
        "Para conectar a VPN sem pedir permissão de administrador a cada vez, o OpenVPN \
         precisa autorizar esta conta do Windows uma única vez (incluindo-a no grupo \
         \"{group}\").\n\nO Windows vai pedir a permissão de um administrador agora. Continuar?",
        "To connect the VPN without asking for administrator permission every time, OpenVPN \
         needs to authorize this Windows account once (adding it to the \"{group}\" \
         group).\n\nWindows will ask for an administrator's permission now. Continue?"
    ));
    let title = super::elevate::wide("Idlerx");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONINFORMATION | MB_SETFOREGROUND,
        ) == IDYES
    }
}

/// Starts openvpn.exe through the service, authorizing the user first when
/// needed (once, with an administrator's approval).
fn start_process(workdir: &Path, options: &str) -> Result<service::Process, Outcome> {
    let fail = |e: Failure| match e {
        Failure::Unavailable if super::installer::openvpn_dir().is_none() => {
            Outcome::MissingOpenVpn
        }
        e => Outcome::Fatal(trf!(
            "Não consegui iniciar o OpenVPN: {}",
            "Could not start OpenVPN: {}",
            e.message()
        )),
    };
    match service::start(workdir, options) {
        Ok(p) => Ok(p),
        Err(Failure::NotAuthorized(msg)) => {
            if !ask_authorization() {
                return Err(Outcome::Fatal(msg));
            }
            service::authorize_current_user().map_err(Outcome::Fatal)?;
            service::start(workdir, options).map_err(fail)
        }
        Err(e) => Err(fail(e)),
    }
}

/// Tunnel up: proves it carries traffic (fetching the public address
/// through it), then opens the gate.
fn tunnel_up(local: Ipv4Addr, log: &Path, gate: &Gate, stop: &AtomicBool) -> Option<Ipv4Addr> {
    // the adapter gets the address a moment after the CONNECTED state
    let mut if_index = None;
    for _ in 0..20 {
        if_index = net::interface_of(local);
        if if_index.is_some() || !pause(Duration::from_millis(250), stop) {
            break;
        }
    }
    let if_index = if_index?;
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let mut dns = pushed_dns(&text);
    // public resolvers as a fallback, also asked through the tunnel
    for ip in [Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(8, 8, 8, 8)] {
        if !dns.contains(&ip) {
            dns.push(ip);
        }
    }
    let route = Route {
        if_index,
        local,
        dns,
    };
    let mut ip = None;
    for _ in 0..10 {
        match proxy::public_ip(&route) {
            Ok(a) => {
                ip = Some(a);
                break;
            }
            Err(_) if pause(Duration::from_secs(1), stop) => {}
            Err(_) => return None,
        }
    }
    gate.open(route);
    ip.or(Some(Ipv4Addr::UNSPECIFIED))
}

fn attempt(
    p: &Params,
    gate: &Arc<Gate>,
    status: &Mutex<Status>,
    stop: &AtomicBool,
    no_dco: bool,
    wake: &Wake,
) -> Outcome {
    let workdir = p.config.parent().map(PathBuf::from).unwrap_or_default();
    let log = log_path(&p.group);
    let _ = std::fs::create_dir_all(log.parent().unwrap_or(Path::new(".")));
    // management password: no other local program can drive this OpenVPN
    let password = random_password();
    let pw_file = super::dir().join(format!("mgmt-{}.txt", p.group));
    if std::fs::write(&pw_file, format!("{password}\n")).is_err() {
        return Outcome::Fatal(
            tr!(
                "Não consegui gravar os arquivos da VPN.",
                "Could not write the VPN files."
            )
            .into(),
        );
    }
    let port = free_port();
    let mut options = vec![
        "--config".to_string(),
        service::arg(&p.config.to_string_lossy()),
        // the server's routes and DNS servers are not for Windows
        "--route-nopull".into(),
        LOW_PRIORITY_ROUTE.into(),
        "--pull-filter ignore \"dhcp-option\" --pull-filter ignore \"dns \"".into(),
        "--pull-filter ignore \"block-outside-dns\" --pull-filter ignore \"route-ipv6\"".into(),
        "--pull-filter ignore \"ifconfig-ipv6\"".into(),
        format!("--management 127.0.0.1 {port}"),
        service::arg(&pw_file.to_string_lossy()),
        "--management-query-passwords --auth-retry interact --auth-nocache".into(),
        "--connect-retry 5 --verb 3 --log".into(),
        service::arg(&log.to_string_lossy()),
    ];
    if no_dco {
        options.push("--disable-dco".into());
    }
    let process = match start_process(&workdir, &options.join(" ")) {
        Ok(pr) => pr,
        Err(outcome) => {
            let _ = std::fs::remove_file(&pw_file);
            return outcome;
        }
    };
    register_process(&p.group, Some(process.pid));

    let mut stream: Option<TcpStream> = None;
    let mut buf: Vec<u8> = Vec::new();
    let mut outcome = Outcome::Exited;
    let mut token_failures = 0u32;
    let mut next_window = false;
    let mut local: Option<Ipv4Addr> = None;
    let mut attempt_since = Instant::now();
    let mut answered = false;
    let mut counted = (
        Instant::now(),
        gate.down.load(Ordering::Relaxed),
        gate.up.load(Ordering::Relaxed),
    );

    // the attempt decided to end (the outcome says why)
    let mut quit = false;
    loop {
        if process.exited().is_some() {
            break;
        }
        if quit || stop.load(Ordering::SeqCst) {
            if !quit {
                outcome = Outcome::Stopped;
            }
            let sent = stream
                .as_mut()
                .is_some_and(|s| s.write_all(b"signal SIGTERM\r\n").is_ok());
            if !sent {
                process.kill();
            }
            break;
        }
        if !answered && attempt_since.elapsed() >= NO_ANSWER {
            update(status, wake, |s| s.slow = true);
            attempt_since = Instant::now();
        }
        // kill switch: the adapter is gone
        if gate.is_open() && local.and_then(net::interface_of).is_none() {
            gate.close();
            update(status, wake, |s| s.phase = Phase::Reconnecting);
        }
        // traffic, every 2 s
        let elapsed = counted.0.elapsed();
        if elapsed >= Duration::from_secs(2) {
            let (down, up) = (
                gate.down.load(Ordering::Relaxed),
                gate.up.load(Ordering::Relaxed),
            );
            let secs = elapsed.as_secs_f64();
            update(status, wake, |s| {
                s.down_rate = down.saturating_sub(counted.1) as f64 / secs;
                s.up_rate = up.saturating_sub(counted.2) as f64 / secs;
                s.down_total = down;
                s.up_total = up;
            });
            counted = (Instant::now(), down, up);
        }

        let s = match stream.as_mut() {
            Some(s) => s,
            None => {
                match TcpStream::connect_timeout(
                    &(Ipv4Addr::LOCALHOST, port).into(),
                    Duration::from_secs(2),
                ) {
                    Ok(s) => {
                        let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                        let st = stream.insert(s);
                        // password, state events (and the current state)
                        let hello = format!("{password}\r\nstate on\r\nstate\r\n");
                        let _ = st.write_all(hello.as_bytes());
                        st
                    }
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                }
            }
        };
        let mut chunk = [0u8; 4096];
        match s.read(&mut chunk) {
            Ok(0) => {
                stream = None;
                continue;
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(_) => {
                stream = None;
                continue;
            }
        }

        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let raw: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&raw).trim().to_string();
            if line.starts_with(">PASSWORD:Need 'Private Key'") {
                outcome = Outcome::Fatal(tr!(
                    "A chave privada deste .ovpn tem senha, o que o Idlerx ainda não suporta.",
                    "This .ovpn private key is password-protected, which Idlerx does not support yet."
                ).into());
                quit = true;
            } else if line.starts_with(">PASSWORD:Need 'Auth'") {
                if !p.auth.uses_user() {
                    outcome = Outcome::Fatal(tr!(
                        "A VPN pediu usuário e senha. Edite o grupo e escolha a autenticação.",
                        "The VPN asked for a username and password. Edit the group and choose the authentication."
                    ).into());
                    quit = true;
                    continue;
                }
                let token = if p.auth.uses_token() {
                    let Some(t) = fresh_token(&p.secret.seed, next_window, stop) else {
                        continue;
                    };
                    t
                } else {
                    String::new()
                };
                next_window = false;
                let pass = if p.auth.uses_password() {
                    format!("{}{token}", p.secret.password)
                } else {
                    token
                };
                if let Some(s) = stream.as_mut() {
                    let msg = format!(
                        "username \"Auth\" \"{}\"\r\npassword \"Auth\" \"{}\"\r\n",
                        escape(&p.username),
                        escape(&pass)
                    );
                    let _ = s.write_all(msg.as_bytes());
                }
            } else if line.starts_with(">PASSWORD:Verification Failed") {
                token_failures += 1;
                next_window = true;
                // a wrong fixed password does not get better by retrying
                let limit = if p.auth.uses_token() {
                    MAX_TOKEN_FAILURES
                } else {
                    1
                };
                if token_failures >= limit {
                    outcome = Outcome::Fatal(tr!(
                        "A VPN recusou o login. Confira o usuário, a senha e o token do grupo.",
                        "The VPN rejected the login. Check the group's username, password and token."
                    ).into());
                    quit = true;
                }
            } else {
                // ">STATE:..." events, or the answer to "state" (current one)
                let current = line
                    .split(',')
                    .next()
                    .is_some_and(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()));
                let Some(rest) = line
                    .strip_prefix(">STATE:")
                    .or(current.then_some(line.as_str()))
                else {
                    continue;
                };
                let parts: Vec<&str> = rest.split(',').collect();
                let state = parts.get(1).copied().unwrap_or("");
                if !no_dco && state == "RECONNECTING" && parts.get(2) == Some(&"dco-connect-error")
                {
                    outcome = Outcome::NoDco;
                    quit = true;
                    continue;
                }
                if matches!(
                    state,
                    "AUTH" | "GET_CONFIG" | "ASSIGN_IP" | "ADD_ROUTES" | "CONNECTED"
                ) {
                    answered = true;
                    update(status, wake, |s| s.slow = false);
                } else if state == "RECONNECTING" {
                    attempt_since = Instant::now();
                    answered = false;
                }
                if state == "CONNECTED" {
                    token_failures = 0;
                    local = parts.get(3).and_then(|s| s.parse().ok());
                    let ip = local.and_then(|l| tunnel_up(l, &log, gate, stop));
                    match ip {
                        Some(ip) => update(status, wake, |s| {
                            s.phase = Phase::Connected;
                            s.ip = (!ip.is_unspecified()).then_some(ip);
                            s.epoch += 1;
                        }),
                        // connected, but nothing goes through: start over
                        None if !stop.load(Ordering::SeqCst) => {
                            if let Some(s) = stream.as_mut() {
                                let _ = s.write_all(b"signal SIGUSR1\r\n");
                            }
                        }
                        None => {}
                    }
                } else {
                    if gate.is_open() {
                        gate.close();
                    }
                    let reconnecting = status.lock().map(|s| s.epoch > 0).unwrap_or(false);
                    update(status, wake, |s| {
                        s.phase = if reconnecting {
                            Phase::Reconnecting
                        } else {
                            Phase::Connecting
                        };
                    });
                }
            }
        }
    }

    gate.close();
    process.wait(Duration::from_secs(15));
    register_process(&p.group, None);
    let _ = std::fs::remove_file(&pw_file);
    if outcome != Outcome::Exited {
        return outcome;
    }
    // it exited by itself: the OpenVPN log explains why
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    if let Some(hwid) = missing_adapter(&text) {
        return Outcome::NeedAdapter(hwid);
    }
    if text.contains("Options error") && !text.contains("PUSH_REPLY") {
        // the .ovpn itself is invalid: retrying does not help
        return Outcome::Fatal(log_errors(&text));
    }
    let errors = log_errors(&text);
    if !errors.is_empty() {
        update(status, wake, |s| s.last_error = Some(errors));
    }
    Outcome::Exited
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_from_the_log() {
        let log =
            "2026-10-12 14:40:35 PUSH: Received control message: 'PUSH_REPLY,redirect-gateway \
                   def1 bypass-dhcp,dhcp-option DNS 8.8.8.8,dhcp-option DNS 8.8.4.4,route-gateway \
                   10.247.83.1,topology subnet'\n\
                   2026-10-12 14:40:35 Pushed option removed by filter: 'dhcp-option DNS 8.8.8.8'";
        assert_eq!(
            pushed_dns(log),
            vec![Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(8, 8, 4, 4)]
        );
        assert!(pushed_dns("nothing here").is_empty());
    }

    #[test]
    fn adapter_missing_from_the_log() {
        let dco = "All ovpn-dco adapters on this system are currently in use or disabled.";
        assert_eq!(missing_adapter(dco), Some("ovpn-dco"));
        let tap = "All tap-windows6 adapters on this system are currently in use or disabled.";
        assert_eq!(missing_adapter(tap), Some("root\\tap0901"));
        assert_eq!(missing_adapter("TLS handshake failed"), None);
    }

    #[test]
    fn errors_from_the_log() {
        let log = "Initialization Sequence Completed\n\
                   ERROR: Cannot resolve host address: vpn.example.com\n\
                   Exiting due to fatal error";
        let e = log_errors(log);
        assert!(e.starts_with("ERROR: Cannot resolve"));
        assert_eq!(e.lines().count(), 2);
    }
}
