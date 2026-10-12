//! The group's gate: a local SOCKS5 proxy whose outgoing connections are
//! pinned to the VPN adapter. The group's browser is told to use only this
//! proxy, so every request of its pages comes through here.
//!
//! Kill switch: while the tunnel is down the gate is closed - new requests
//! are refused and the open connections are cut on the spot. There is no
//! path to the regular network: a socket pinned to a missing adapter fails
//! instead of choosing another route. Names are resolved through the tunnel
//! too (`dns`), and IPv6 is refused (the tunnel carries only IPv4).

use super::dns;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const DNS_CACHE: Duration = Duration::from_secs(60);

/// Way out while the tunnel is up.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    /// Index of the VPN network adapter and the address it got.
    pub if_index: u32,
    pub local: Ipv4Addr,
    /// DNS servers asked through the tunnel, in order.
    pub dns: Vec<Ipv4Addr>,
}

#[derive(Default)]
struct Inner {
    route: Option<Route>,
    /// Increases on every open/close: a connection made under an older one
    /// does not get registered (it is cut right away).
    epoch: u64,
    /// Both sides of every relayed connection, to cut them all at once.
    open: HashMap<u64, (TcpStream, TcpStream)>,
    names: HashMap<String, (Vec<Ipv4Addr>, Instant)>,
}

/// One gate per group with a VPN.
#[derive(Default)]
pub struct Gate {
    inner: Mutex<Inner>,
    next_id: AtomicU64,
    /// Bytes received from the sites (download) and sent to them (upload).
    pub down: AtomicU64,
    pub up: AtomicU64,
}

impl Gate {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Tunnel up: requests go out through `route`.
    pub fn open(&self, route: Route) {
        let mut g = self.lock();
        if g.route.as_ref() != Some(&route) {
            g.epoch += 1;
            g.names.clear();
            g.route = Some(route);
        }
    }

    /// Tunnel down: refuses new requests and cuts every open connection.
    pub fn close(&self) {
        let open = {
            let mut g = self.lock();
            g.route = None;
            g.epoch += 1;
            g.names.clear();
            std::mem::take(&mut g.open)
        };
        for (client, remote) in open.values() {
            let _ = client.shutdown(Shutdown::Both);
            let _ = remote.shutdown(Shutdown::Both);
        }
    }

    pub fn is_open(&self) -> bool {
        self.lock().route.is_some()
    }

    /// Connections being relayed right now.
    #[cfg(test)]
    pub fn connections(&self) -> usize {
        self.lock().open.len()
    }

    fn route(&self) -> Option<(Route, u64)> {
        let g = self.lock();
        g.route.clone().map(|r| (r, g.epoch))
    }

    fn lookup(&self, name: &str, route: &Route) -> io::Result<Vec<Ipv4Addr>> {
        let key = name.to_ascii_lowercase();
        if let Some((ips, at)) = self.lock().names.get(&key) {
            if at.elapsed() < DNS_CACHE {
                return Ok(ips.clone());
            }
        }
        let ips = dns::resolve(&key, &route.dns, route.if_index, route.local)?;
        self.lock().names.insert(key, (ips.clone(), Instant::now()));
        Ok(ips)
    }

    /// Starts listening on 127.0.0.1 (a free port, returned).
    pub fn serve(self: &Arc<Self>) -> io::Result<u16> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let gate = self.clone();
        std::thread::Builder::new()
            .name("vpn-gate".into())
            .spawn(move || {
                for client in listener.incoming().flatten() {
                    let local = client.peer_addr().is_ok_and(|a| a.ip().is_loopback());
                    if !local {
                        continue;
                    }
                    let gate = gate.clone();
                    let _ = std::thread::Builder::new()
                        .name("vpn-conn".into())
                        .spawn(move || {
                            let _ = gate.handle(client);
                        });
                }
            })?;
        Ok(port)
    }

    fn handle(self: &Arc<Self>, mut client: TcpStream) -> io::Result<()> {
        client.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
        let target = match read_request(&mut client)? {
            Ok(t) => t,
            Err(reply) => return send_reply(&mut client, reply),
        };
        let Some((route, epoch)) = self.route() else {
            return send_reply(&mut client, REPLY_NETWORK_UNREACHABLE);
        };
        let ips = match target.host {
            Host::Ip(ip) => vec![ip],
            Host::Name(ref name) => match self.lookup(name, &route) {
                Ok(ips) => ips,
                Err(_) => return send_reply(&mut client, REPLY_HOST_UNREACHABLE),
            },
        };
        if ips.iter().any(|ip| ip.is_loopback() || ip.is_unspecified()) {
            return send_reply(&mut client, REPLY_NOT_ALLOWED);
        }
        let mut remote = None;
        for ip in ips.iter().take(3) {
            let addr = SocketAddrV4::new(*ip, target.port);
            if let Ok(s) = connect_pinned(addr, route.if_index, route.local) {
                remote = Some(s);
                break;
            }
        }
        let Some(remote) = remote else {
            return send_reply(&mut client, REPLY_HOST_UNREACHABLE);
        };

        // register before replying; the tunnel may have dropped meanwhile
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        {
            let mut g = self.lock();
            if g.epoch != epoch || g.route.is_none() {
                drop(g);
                let _ = remote.shutdown(Shutdown::Both);
                return send_reply(&mut client, REPLY_NETWORK_UNREACHABLE);
            }
            g.open
                .insert(id, (client.try_clone()?, remote.try_clone()?));
        }
        send_reply(&mut client, REPLY_OK)?;
        client.set_read_timeout(None)?;

        let gate = self.clone();
        let (c2, r2) = (client.try_clone()?, remote.try_clone()?);
        let upload = std::thread::Builder::new()
            .name("vpn-up".into())
            .spawn(move || relay(c2, r2, &gate.up));
        relay(remote, client, &self.down);
        if let Ok(t) = upload {
            let _ = t.join();
        }
        self.lock().open.remove(&id);
        Ok(())
    }
}

/// Address the sites see through the tunnel (asked to api.ipify.org through
/// it). Also proves the tunnel really carries traffic before the gate opens.
pub fn public_ip(route: &Route) -> io::Result<Ipv4Addr> {
    let ips = dns::resolve("api.ipify.org", &route.dns, route.if_index, route.local)?;
    let addr = SocketAddrV4::new(ips[0], 80);
    let mut s = connect_pinned(addr, route.if_index, route.local)?;
    s.set_read_timeout(Some(Duration::from_secs(8)))?;
    s.write_all(b"GET / HTTP/1.1\r\nHost: api.ipify.org\r\nConnection: close\r\n\r\n")?;
    let mut answer = String::new();
    s.take(8192).read_to_string(&mut answer)?;
    let body = answer.split_once("\r\n\r\n").map_or("", |(_, b)| b);
    body.lines()
        .find_map(|l| l.trim().parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no address in the answer"))
}

/// TCP connection that can only leave through the given adapter.
fn connect_pinned(addr: SocketAddrV4, if_index: u32, local: Ipv4Addr) -> io::Result<TcpStream> {
    use socket2::{Domain, Protocol, Socket, Type};
    use std::os::windows::io::AsRawSocket;
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    // with the tunnel gone the address no longer exists: binding fails
    socket.bind(&SocketAddr::V4(SocketAddrV4::new(local, 0)).into())?;
    dns::pin_to_interface(socket.as_raw_socket(), if_index)?;
    socket.connect_timeout(&SocketAddr::V4(addr).into(), CONNECT_TIMEOUT)?;
    let stream: TcpStream = socket.into();
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// Copies one direction until it ends, then closes both sides.
fn relay(mut from: TcpStream, mut to: TcpStream, counter: &AtomicU64) {
    let mut buf = [0u8; 16 * 1024];
    loop {
        match from.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                counter.fetch_add(n as u64, Ordering::Relaxed);
                if to.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        }
    }
    let _ = from.shutdown(Shutdown::Both);
    let _ = to.shutdown(Shutdown::Both);
}

// ------------------------------------------------------------- SOCKS5 --

const REPLY_OK: u8 = 0;
const REPLY_NOT_ALLOWED: u8 = 2;
const REPLY_NETWORK_UNREACHABLE: u8 = 3;
const REPLY_HOST_UNREACHABLE: u8 = 4;
const REPLY_COMMAND_NOT_SUPPORTED: u8 = 7;
const REPLY_ADDRESS_NOT_SUPPORTED: u8 = 8;

#[derive(Debug, PartialEq)]
enum Host {
    Ip(Ipv4Addr),
    Name(String),
}

#[derive(Debug, PartialEq)]
struct Target {
    host: Host,
    port: u16,
}

/// Reads the greeting and the CONNECT request. Inner Err = SOCKS reply code
/// to refuse with.
fn read_request(s: &mut impl ReadWrite) -> io::Result<Result<Target, u8>> {
    let mut head = [0u8; 2];
    s.read_exact(&mut head)?;
    if head[0] != 5 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not SOCKS5"));
    }
    let mut methods = vec![0u8; head[1] as usize];
    s.read_exact(&mut methods)?;
    if !methods.contains(&0) {
        s.write_all(&[5, 0xFF])?; // only "no authentication"
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "no usable method",
        ));
    }
    s.write_all(&[5, 0])?;

    let mut req = [0u8; 4];
    s.read_exact(&mut req)?;
    if req[0] != 5 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not SOCKS5"));
    }
    let host = match req[3] {
        1 => {
            let mut ip = [0u8; 4];
            s.read_exact(&mut ip)?;
            Host::Ip(Ipv4Addr::from(ip))
        }
        3 => {
            let mut len = [0u8; 1];
            s.read_exact(&mut len)?;
            let mut name = vec![0u8; len[0] as usize];
            s.read_exact(&mut name)?;
            let name = String::from_utf8_lossy(&name).into_owned();
            // "[::1]" style literals, or a name that is an IPv6 address
            if name.contains(':') {
                read_port(s)?;
                return Ok(Err(REPLY_ADDRESS_NOT_SUPPORTED));
            }
            match name.parse::<Ipv4Addr>() {
                Ok(ip) => Host::Ip(ip),
                Err(_) => Host::Name(name),
            }
        }
        4 => {
            // IPv6 is blocked: the tunnel carries only IPv4
            let mut ip = [0u8; 16];
            s.read_exact(&mut ip)?;
            read_port(s)?;
            return Ok(Err(REPLY_ADDRESS_NOT_SUPPORTED));
        }
        _ => return Ok(Err(REPLY_ADDRESS_NOT_SUPPORTED)),
    };
    let port = read_port(s)?;
    if req[1] != 1 {
        // only CONNECT: no BIND, no UDP (QUIC/WebRTC never leave this way)
        return Ok(Err(REPLY_COMMAND_NOT_SUPPORTED));
    }
    Ok(Ok(Target { host, port }))
}

fn read_port(s: &mut impl Read) -> io::Result<u16> {
    let mut p = [0u8; 2];
    s.read_exact(&mut p)?;
    Ok(u16::from_be_bytes(p))
}

fn send_reply(s: &mut impl Write, code: u8) -> io::Result<()> {
    s.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0])
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory stream: reads `input`, collects what is written.
    struct Mock {
        input: io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for Mock {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.input.read(b)
        }
    }
    impl Write for Mock {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    fn mock(bytes: &[u8]) -> Mock {
        Mock {
            input: io::Cursor::new(bytes.to_vec()),
            output: Vec::new(),
        }
    }

    #[test]
    fn socks_requests() {
        // domain name, port 443
        let mut m = mock(&[
            5, 1, 0, 5, 1, 0, 3, 5, b'a', b'.', b'c', b'o', b'm', 1, 0xBB,
        ]);
        let t = read_request(&mut m).unwrap().unwrap();
        assert_eq!(
            t,
            Target {
                host: Host::Name("a.com".into()),
                port: 443
            }
        );
        assert_eq!(m.output, [5, 0]);

        // IPv4 literal sent as a name
        let mut m = mock(&[
            5, 1, 0, 5, 1, 0, 3, 7, b'1', b'.', b'2', b'.', b'3', b'.', b'4', 0, 80,
        ]);
        let t = read_request(&mut m).unwrap().unwrap();
        assert_eq!(t.host, Host::Ip(Ipv4Addr::new(1, 2, 3, 4)));

        // IPv6: refused
        let mut v6 = vec![5, 1, 0, 5, 1, 0, 4];
        v6.extend_from_slice(&[0; 16]);
        v6.extend_from_slice(&[0, 80]);
        assert_eq!(
            read_request(&mut mock(&v6)).unwrap(),
            Err(REPLY_ADDRESS_NOT_SUPPORTED)
        );
        let mut m = mock(&[5, 1, 0, 5, 1, 0, 3, 3, b':', b':', b'1', 0, 80]);
        assert_eq!(
            read_request(&mut m).unwrap(),
            Err(REPLY_ADDRESS_NOT_SUPPORTED)
        );

        // UDP ASSOCIATE: refused
        let mut m = mock(&[5, 1, 0, 5, 3, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            read_request(&mut m).unwrap(),
            Err(REPLY_COMMAND_NOT_SUPPORTED)
        );

        // authentication only: refused
        let mut m = mock(&[5, 1, 2]);
        assert!(read_request(&mut m).is_err());
        assert_eq!(m.output, [5, 0xFF]);
    }

    /// Closed gate: the request is refused without connecting anywhere.
    #[test]
    fn closed_gate_refuses() {
        let gate = Arc::new(Gate::default());
        let port = gate.serve().unwrap();
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        s.write_all(&[5, 1, 0, 5, 1, 0, 1, 93, 184, 215, 14, 0, 80])
            .unwrap();
        let mut r = [0u8; 12];
        s.read_exact(&mut r).unwrap();
        assert_eq!(r[..2], [5, 0]);
        assert_eq!(r[2..4], [5, REPLY_NETWORK_UNREACHABLE]);
    }

    /// Open connections are cut when the gate closes.
    #[test]
    fn close_cuts_connections() {
        // a local "site" on the loopback adapter
        let site = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let site_addr = site.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut c, _) = site.accept().unwrap();
            let mut b = [0u8; 4];
            let _ = c.read(&mut b);
            let _ = c.write_all(b"pong");
            std::thread::sleep(Duration::from_secs(30));
        });
        let gate = Arc::new(Gate::default());
        // straight to the relay: loopback targets are refused by handle()
        let (client_side, gate_side) = {
            let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let a = TcpStream::connect(l.local_addr().unwrap()).unwrap();
            (a, l.accept().unwrap().0)
        };
        let remote = TcpStream::connect(site_addr).unwrap();
        gate.open(Route {
            if_index: 1,
            local: Ipv4Addr::LOCALHOST,
            dns: vec![],
        });
        gate.lock().open.insert(
            0,
            (gate_side.try_clone().unwrap(), remote.try_clone().unwrap()),
        );
        let g2 = gate.clone();
        let (gs, rs) = (gate_side.try_clone().unwrap(), remote.try_clone().unwrap());
        std::thread::spawn(move || relay(gs, rs, &g2.up));
        let g3 = gate.clone();
        std::thread::spawn(move || relay(remote, gate_side, &g3.down));

        let mut client = client_side;
        client.write_all(b"ping").unwrap();
        let mut b = [0u8; 4];
        client.read_exact(&mut b).unwrap();
        assert_eq!(&b, b"pong");
        assert_eq!(gate.down.load(Ordering::Relaxed), 4);
        assert_eq!(gate.connections(), 1);

        gate.close();
        assert!(!gate.is_open());
        assert_eq!(gate.connections(), 0);
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        assert_eq!(client.read(&mut b).unwrap_or(0), 0, "connection still open");
    }
}
