//! A reader for LevelDB folders, enough to bring Chromium's localStorage
//! (where many browser games keep their saves) over to Firefox: the write
//! log (`*.log`) and the tables (`*.ldb`, blocks maybe compressed with
//! Snappy). Every entry is read with its sequence number; the newest one of
//! each key wins and deleted keys are left out.

use std::collections::HashMap;
use std::path::Path;

/// Key -> value, as stored.
pub type Entries = HashMap<Vec<u8>, Vec<u8>>;

/// (sequence, value; None = deleted) of each key seen so far.
type Seen = HashMap<Vec<u8>, (u64, Option<Vec<u8>>)>;

fn put(seen: &mut Seen, key: Vec<u8>, seq: u64, value: Option<Vec<u8>>) {
    match seen.get(&key) {
        Some((s, _)) if *s >= seq => {}
        _ => {
            seen.insert(key, (seq, value));
        }
    }
}

fn varint(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut out = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *data.get(*pos)?;
        *pos += 1;
        out |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(out);
        }
    }
    None
}

fn bytes<'a>(data: &'a [u8], pos: &mut usize, n: usize) -> Option<&'a [u8]> {
    let s = data.get(*pos..*pos + n)?;
    *pos += n;
    Some(s)
}

/// Reads a whole LevelDB folder (a copy: the original may be in use).
pub fn read_dir(dir: &Path) -> Entries {
    let mut seen = Seen::new();
    let Ok(files) = std::fs::read_dir(dir) else {
        return Entries::new();
    };
    for f in files.flatten() {
        let name = f.file_name().to_string_lossy().to_string();
        let Ok(data) = std::fs::read(f.path()) else {
            continue;
        };
        if name.ends_with(".log") {
            read_log(&data, &mut seen);
        } else if name.ends_with(".ldb") || name.ends_with(".sst") {
            read_table(&data, &mut seen);
        }
    }
    seen.into_iter()
        .filter_map(|(k, (_, v))| v.map(|v| (k, v)))
        .collect()
}

/// The write log: 32 KB blocks of records (whole or in pieces), each a
/// batch of writes.
fn read_log(data: &[u8], seen: &mut Seen) {
    const BLOCK: usize = 32 * 1024;
    let mut pos = 0;
    let mut record = Vec::new();
    while pos + 7 <= data.len() {
        let left = BLOCK - pos % BLOCK;
        if left < 7 {
            pos += left;
            continue;
        }
        let len = u16::from_le_bytes([data[pos + 4], data[pos + 5]]) as usize;
        let kind = data[pos + 6];
        let start = pos + 7;
        let Some(piece) = data.get(start..start + len) else {
            break;
        };
        pos = start + len;
        match kind {
            1 => batch(piece, seen),
            2 => record = piece.to_vec(),
            3 => record.extend_from_slice(piece),
            4 => {
                record.extend_from_slice(piece);
                batch(&record, seen);
                record.clear();
            }
            _ => {}
        }
    }
}

fn batch(data: &[u8], seen: &mut Seen) {
    if data.len() < 12 {
        return;
    }
    let seq = u64::from_le_bytes(data[..8].try_into().unwrap_or_default());
    let count = u32::from_le_bytes(data[8..12].try_into().unwrap_or_default());
    let mut pos = 12;
    for i in 0..count as u64 {
        let Some(&tag) = data.get(pos) else { return };
        pos += 1;
        let Some(klen) = varint(data, &mut pos) else {
            return;
        };
        let Some(key) = bytes(data, &mut pos, klen as usize) else {
            return;
        };
        let key = key.to_vec();
        if tag == 1 {
            let Some(vlen) = varint(data, &mut pos) else {
                return;
            };
            let Some(value) = bytes(data, &mut pos, vlen as usize) else {
                return;
            };
            put(seen, key, seq + i, Some(value.to_vec()));
        } else {
            put(seen, key, seq + i, None);
        }
    }
}

/// A table: its footer points to the index block, whose entries point to
/// the data blocks.
fn read_table(data: &[u8], seen: &mut Seen) {
    if data.len() < 48 {
        return;
    }
    let footer = &data[data.len() - 48..];
    let mut pos = 0;
    let (Some(_), Some(_)) = (varint(footer, &mut pos), varint(footer, &mut pos)) else {
        return;
    };
    let (Some(off), Some(size)) = (varint(footer, &mut pos), varint(footer, &mut pos)) else {
        return;
    };
    let Some(index) = block(data, off as usize, size as usize) else {
        return;
    };
    for (_, handle) in entries(&index) {
        let mut p = 0;
        let (Some(off), Some(size)) = (varint(&handle, &mut p), varint(&handle, &mut p)) else {
            continue;
        };
        let Some(b) = block(data, off as usize, size as usize) else {
            continue;
        };
        for (ikey, value) in entries(&b) {
            // internal key: the key, then 8 bytes of sequence << 8 | kind
            if ikey.len() < 8 {
                continue;
            }
            let (key, trailer) = ikey.split_at(ikey.len() - 8);
            let t = u64::from_le_bytes(trailer.try_into().unwrap_or_default());
            let value = (t & 0xff == 1).then_some(value);
            put(seen, key.to_vec(), t >> 8, value);
        }
    }
}

/// A block's contents (uncompressed), without its trailer.
fn block(data: &[u8], off: usize, size: usize) -> Option<Vec<u8>> {
    let raw = data.get(off..off + size)?;
    let kind = *data.get(off + size)?;
    match kind {
        0 => Some(raw.to_vec()),
        1 => snap::raw::Decoder::new().decompress_vec(raw).ok(),
        _ => None,
    }
}

/// The (key, value) entries of a block (keys share prefixes with the
/// previous one).
fn entries(b: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    if b.len() < 4 {
        return out;
    }
    let restarts = u32::from_le_bytes(b[b.len() - 4..].try_into().unwrap_or_default()) as usize;
    let end = b.len().saturating_sub(4 + restarts * 4);
    let data = &b[..end];
    let mut pos = 0;
    let mut key: Vec<u8> = Vec::new();
    while pos < data.len() {
        let (Some(shared), Some(unshared), Some(vlen)) = (
            varint(data, &mut pos),
            varint(data, &mut pos),
            varint(data, &mut pos),
        ) else {
            break;
        };
        let Some(rest) = bytes(data, &mut pos, unshared as usize) else {
            break;
        };
        key.truncate(shared as usize);
        key.extend_from_slice(rest);
        let Some(value) = bytes(data, &mut pos, vlen as usize) else {
            break;
        };
        out.push((key.clone(), value.to_vec()));
    }
    out
}

/// Chromium's localStorage entries: origin -> (key, value), from the raw
/// LevelDB entries ("_" + origin + "\0" + key; keys and values start with a
/// byte saying their encoding: 0 = UTF-16LE, 1 = Latin-1).
pub fn local_storage(entries: &Entries) -> HashMap<String, Vec<(String, String)>> {
    let text = |b: &[u8]| -> Option<String> {
        let (&kind, rest) = b.split_first()?;
        match kind {
            0 => {
                let units: Vec<u16> = rest
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .collect();
                String::from_utf16(&units).ok()
            }
            1 => Some(rest.iter().map(|&c| c as char).collect()),
            _ => None,
        }
    };
    let mut out: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (k, v) in entries {
        let Some(rest) = k.strip_prefix(b"_") else {
            continue;
        };
        let Some(zero) = rest.iter().position(|&c| c == 0) else {
            continue;
        };
        let origin = String::from_utf8_lossy(&rest[..zero]).to_string();
        let (Some(key), Some(value)) = (text(&rest[zero + 1..]), text(v)) else {
            continue;
        };
        out.entry(origin).or_default().push((key, value));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint_bytes(mut n: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(b);
                return out;
            }
            out.push(b | 0x80);
        }
    }

    fn batch_bytes(seq: u64, writes: &[(&[u8], Option<&[u8]>)]) -> Vec<u8> {
        let mut b = seq.to_le_bytes().to_vec();
        b.extend_from_slice(&(writes.len() as u32).to_le_bytes());
        for (k, v) in writes {
            b.push(v.is_some() as u8);
            b.extend(varint_bytes(k.len() as u64));
            b.extend_from_slice(k);
            if let Some(v) = v {
                b.extend(varint_bytes(v.len() as u64));
                b.extend_from_slice(v);
            }
        }
        b
    }

    fn record(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut r = vec![0, 0, 0, 0];
        r.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        r.push(kind);
        r.extend_from_slice(payload);
        r
    }

    #[test]
    fn log_batches_newest_wins() {
        let mut log = record(
            1,
            &batch_bytes(1, &[(b"a", Some(b"1")), (b"b", Some(b"x"))]),
        );
        log.extend(record(
            1,
            &batch_bytes(3, &[(b"a", Some(b"2")), (b"b", None)]),
        ));
        let mut seen = Seen::new();
        read_log(&log, &mut seen);
        let e: Entries = seen
            .into_iter()
            .filter_map(|(k, (_, v))| v.map(|v| (k, v)))
            .collect();
        assert_eq!(
            e.get(b"a".as_slice()).map(|v| v.as_slice()),
            Some(b"2".as_slice())
        );
        assert!(!e.contains_key(b"b".as_slice()));
    }

    #[test]
    fn block_entries_share_prefixes() {
        // "apple"=1, "apply"=2 (shares "appl"), one restart point
        let mut b = Vec::new();
        b.extend([0, 5, 1]);
        b.extend_from_slice(b"apple1");
        b.extend([4, 1, 1]);
        b.extend_from_slice(b"y2");
        b.extend(0u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        let e = entries(&b);
        assert_eq!(e[0], (b"apple".to_vec(), b"1".to_vec()));
        assert_eq!(e[1], (b"apply".to_vec(), b"2".to_vec()));
    }

    #[test]
    fn chromium_local_storage_keys() {
        let mut e = Entries::new();
        let mut k = b"_https://game.example\0\x01save".to_vec();
        e.insert(k.clone(), b"\x01{\"gold\":1}".to_vec());
        k = b"_https://game.example\0\x00".to_vec();
        k.extend("é".encode_utf16().flat_map(|u| u.to_le_bytes()));
        let mut v = vec![0u8];
        v.extend("ação🎮".encode_utf16().flat_map(|u| u.to_le_bytes()));
        e.insert(k, v);
        e.insert(b"META:https://game.example".to_vec(), b"x".to_vec());
        let ls = local_storage(&e);
        let mut items = ls["https://game.example"].clone();
        items.sort();
        assert_eq!(
            items,
            vec![
                ("save".to_string(), "{\"gold\":1}".to_string()),
                ("é".to_string(), "ação🎮".to_string()),
            ]
        );
        assert_eq!(ls.len(), 1);
    }
}
