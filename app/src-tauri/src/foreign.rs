//! Reading other apps' SQLite files without writing next to them, and just
//! enough protobuf to pull known fields out of blobs with no published schema.
//! Mirrors `wrapped/src/sqlite.mjs` and `wrapped/src/protobuf.mjs`.

use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// A plain read-only open of a WAL-mode database creates "-wal" and "-shm"
/// files beside it when they do not exist yet. If the owning app is live (its
/// -wal exists), open read-only to see its latest writes; otherwise open as
/// immutable, which writes nothing.
pub fn open_foreign_db(path: &Path) -> Option<Connection> {
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI;
    if Path::new(&wal).exists() {
        Connection::open_with_flags(path, flags).ok()
    } else {
        Connection::open_with_flags(immutable_uri(path), flags).ok()
    }
}

fn immutable_uri(path: &Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::from(if p.starts_with('/') { "file://" } else { "file:///" });
    for c in p.chars() {
        match c {
            '%' => uri.push_str("%25"),
            '?' => uri.push_str("%3f"),
            '#' => uri.push_str("%23"),
            ' ' => uri.push_str("%20"),
            _ => uri.push(c),
        }
    }
    uri.push_str("?immutable=1");
    uri
}

pub enum Value<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

struct Field<'a> {
    no: u64,
    value: Value<'a>,
}

fn varint(buf: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let mut v: u64 = 0;
    for shift in (0..64).step_by(7) {
        let b = *buf.get(i)?;
        i += 1;
        v |= u64::from(b & 0x7f) << shift;
        if b < 0x80 {
            return Some((v, i));
        }
    }
    None
}

fn fields(buf: &[u8]) -> Option<Vec<Field<'_>>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        let (key, j) = varint(buf, i)?;
        i = j;
        let value = match key & 7 {
            0 => {
                let (v, k) = varint(buf, i)?;
                i = k;
                Value::Int(v)
            }
            2 => {
                let (len, k) = varint(buf, i)?;
                let end = k.checked_add(usize::try_from(len).ok()?)?;
                let b = buf.get(k..end)?;
                i = end;
                Value::Bytes(b)
            }
            1 => {
                let b = buf.get(i..i + 8)?;
                i += 8;
                Value::Bytes(b)
            }
            5 => {
                let b = buf.get(i..i + 4)?;
                i += 4;
                Value::Bytes(b)
            }
            _ => return None,
        };
        out.push(Field { no: key >> 3, value });
    }
    Some(out)
}

/// First value at a path of field numbers.
pub fn get<'a>(buf: &'a [u8], path: &[u64]) -> Option<Value<'a>> {
    let (&last, parents) = path.split_last()?;
    let mut cur = buf;
    for &no in parents {
        cur = match fields(cur)?.into_iter().find(|f| f.no == no)?.value {
            Value::Bytes(b) => b,
            Value::Int(_) => return None,
        };
    }
    fields(cur)?.into_iter().find(|f| f.no == last).map(|f| f.value)
}

pub fn get_string(buf: &[u8], path: &[u64]) -> Option<String> {
    match get(buf, path)? {
        Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
        Value::Int(_) => None,
    }
}

fn get_int(buf: &[u8], path: &[u64]) -> Option<u64> {
    match get(buf, path)? {
        Value::Int(v) => Some(v),
        Value::Bytes(_) => None,
    }
}

/// google.protobuf.Timestamp { 1: seconds, 2: nanos } at path, in ms.
pub fn get_timestamp(buf: &[u8], path: &[u64]) -> Option<i64> {
    let msg = match get(buf, path)? {
        Value::Bytes(b) => b,
        Value::Int(_) => return None,
    };
    let seconds = i64::try_from(get_int(msg, &[1])?).ok()?;
    let nanos = get_int(msg, &[2]).unwrap_or(0);
    Some(seconds * 1000 + i64::try_from(nanos / 1_000_000).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immutable_uri_escapes_and_handles_windows() {
        assert_eq!(immutable_uri(Path::new("/a b/x?.db")), "file:///a%20b/x%3f.db?immutable=1");
        assert_eq!(immutable_uri(Path::new("C:\\Users\\me\\s.db")), "file:///C:/Users/me/s.db?immutable=1");
    }

    #[test]
    fn reads_nested_fields() {
        // { 1: { 1: 1789200000, 2: 500000000 }, 4: { 2: "run_command" } }
        let buf = [
            0x0a, 0x0c, 0x08, 0x80, 0x8d, 0x94, 0xd5, 0x06, 0x10, 0x80, 0xca, 0xb5, 0xee, 0x01, 0x22, 0x0d, 0x12, 0x0b, b'r', b'u',
            b'n', b'_', b'c', b'o', b'm', b'm', b'a', b'n', b'd',
        ];
        assert_eq!(get_timestamp(&buf, &[1]), Some(1_789_200_000_500));
        assert_eq!(get_string(&buf, &[4, 2]).as_deref(), Some("run_command"));
        assert!(get(&buf, &[9]).is_none());
        assert!(get_string(&[0xff, 0xff], &[1]).is_none());
    }
}
