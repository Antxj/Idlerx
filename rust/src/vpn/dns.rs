//! Name resolution through the tunnel: plain DNS over UDP (A records only),
//! sent out of the VPN adapter. The system resolver is never used for the
//! sites of a group with a VPN, so lookups cannot leak outside the tunnel.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(3);
const TYPE_A: u16 = 1;
const CLASS_IN: u16 = 1;

/// Pins a socket to one network interface (IPv4): it can only send through
/// it, whatever the routing table says. With the interface gone, sending
/// fails - it never falls back to another one. The socket must also be
/// bound to the interface's address, or Windows may pick the source
/// address of another network.
pub fn pin_to_interface(socket: std::os::windows::io::RawSocket, if_index: u32) -> io::Result<()> {
    use windows_sys::Win32::Networking::WinSock::{setsockopt, IPPROTO_IP, IP_UNICAST_IF};
    // IP_UNICAST_IF takes the index in network byte order
    let value = if_index.to_be();
    let r = unsafe {
        setsockopt(
            socket as usize,
            IPPROTO_IP,
            IP_UNICAST_IF,
            &value as *const u32 as *const u8,
            4,
        )
    };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// IPv4 addresses of `name`, asking `servers` in order through the
/// interface `if_index`, whose address is `local`.
pub fn resolve(
    name: &str,
    servers: &[Ipv4Addr],
    if_index: u32,
    local: Ipv4Addr,
) -> io::Result<Vec<Ipv4Addr>> {
    if let Ok(ip) = name.parse::<Ipv4Addr>() {
        return Ok(vec![ip]);
    }
    let query = build_query(name, random_id())?;
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no DNS server");
    for server in servers {
        match ask(&query, *server, if_index, local) {
            Ok(answer) => return parse_answer(&answer, &query),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn ask(query: &[u8], server: Ipv4Addr, if_index: u32, local: Ipv4Addr) -> io::Result<Vec<u8>> {
    use std::os::windows::io::AsRawSocket;
    let socket = UdpSocket::bind((local, 0))?;
    pin_to_interface(socket.as_raw_socket(), if_index)?;
    socket.set_read_timeout(Some(TIMEOUT))?;
    socket.send_to(query, SocketAddr::from((server, 53)))?;
    let mut buf = [0u8; 1500];
    loop {
        let (n, from) = socket.recv_from(&mut buf)?;
        // only the server asked, with the same query id
        if from == SocketAddr::from((server, 53)) && n >= 2 && buf[..2] == query[..2] {
            return Ok(buf[..n].to_vec());
        }
    }
}

fn random_id() -> [u8; 2] {
    let mut id = [0u8; 2];
    let _ = getrandom::fill(&mut id);
    id
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Standard recursive query for the A record of `name`.
fn build_query(name: &str, id: [u8; 2]) -> io::Result<Vec<u8>> {
    let name = name.trim_end_matches('.');
    if name.is_empty() || name.len() > 253 {
        return Err(invalid("bad name"));
    }
    let mut q = Vec::with_capacity(name.len() + 18);
    q.extend_from_slice(&id);
    q.extend_from_slice(&[0x01, 0x00]); // recursion desired
    q.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]); // 1 question
    for label in name.split('.') {
        if label.is_empty() || label.len() > 63 || !label.is_ascii() {
            return Err(invalid("bad name"));
        }
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&TYPE_A.to_be_bytes());
    q.extend_from_slice(&CLASS_IN.to_be_bytes());
    Ok(q)
}

/// Skips a (possibly compressed) name; returns the offset after it.
fn skip_name(msg: &[u8], mut pos: usize) -> io::Result<usize> {
    loop {
        let len = *msg.get(pos).ok_or_else(|| invalid("short"))? as usize;
        match len {
            0 => return Ok(pos + 1),
            l if l & 0xC0 == 0xC0 => return Ok(pos + 2), // pointer
            l => pos += 1 + l,
        }
    }
}

fn u16_at(msg: &[u8], pos: usize) -> io::Result<u16> {
    msg.get(pos..pos + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or_else(|| invalid("short"))
}

fn parse_answer(msg: &[u8], query: &[u8]) -> io::Result<Vec<Ipv4Addr>> {
    if msg.len() < 12 || msg[..2] != query[..2] || msg[2] & 0x80 == 0 {
        return Err(invalid("not an answer"));
    }
    let rcode = msg[3] & 0x0F;
    if rcode == 3 {
        return Err(io::Error::new(io::ErrorKind::NotFound, "no such name"));
    }
    if rcode != 0 {
        return Err(invalid("server failure"));
    }
    let questions = u16_at(msg, 4)?;
    let answers = u16_at(msg, 6)?;
    let mut pos = 12;
    for _ in 0..questions {
        pos = skip_name(msg, pos)? + 4;
    }
    let mut ips = Vec::new();
    for _ in 0..answers {
        pos = skip_name(msg, pos)?;
        let kind = u16_at(msg, pos)?;
        let class = u16_at(msg, pos + 2)?;
        let len = u16_at(msg, pos + 8)? as usize;
        let data = msg
            .get(pos + 10..pos + 10 + len)
            .ok_or_else(|| invalid("short"))?;
        if kind == TYPE_A && class == CLASS_IN && len == 4 {
            ips.push(Ipv4Addr::new(data[0], data[1], data[2], data[3]));
        }
        pos += 10 + len;
    }
    if ips.is_empty() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "no IPv4 address"));
    }
    Ok(ips)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_format() {
        let q = build_query("a.bc", [0x12, 0x34]).unwrap();
        assert_eq!(
            q,
            [0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, b'a', 2, b'b', b'c', 0, 0, 1, 0, 1]
        );
        assert!(build_query("", [0, 0]).is_err());
        assert!(build_query("a..b", [0, 0]).is_err());
        assert!(build_query("ação.com", [0, 0]).is_err());
    }

    #[test]
    fn answer_with_cname_and_compression() {
        let q = build_query("www.x.io", [7, 7]).unwrap();
        let mut a = q.clone();
        a[2] = 0x81; // answer, recursion desired
        a[3] = 0x80;
        a[7] = 2; // 2 answers
                  // CNAME www.x.io -> x.io (pointer to offset 16)
        a.extend_from_slice(&[0xC0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 2, 0xC0, 16]);
        // A x.io 203.0.113.9
        a.extend_from_slice(&[0xC0, 16, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 203, 0, 113, 9]);
        assert_eq!(
            parse_answer(&a, &q).unwrap(),
            vec![Ipv4Addr::new(203, 0, 113, 9)]
        );

        let mut wrong_id = a.clone();
        wrong_id[0] = 9;
        assert!(parse_answer(&wrong_id, &q).is_err());
        let mut nx = q.clone();
        nx[2] = 0x81;
        nx[3] = 0x83;
        assert_eq!(
            parse_answer(&nx, &q).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(parse_answer(&a[..a.len() - 2], &q).is_err());
    }
}
