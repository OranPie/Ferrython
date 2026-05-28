use super::*;
use std::collections::HashMap;

#[derive(Default)]
pub(in crate::serial_modules) struct PickleWriteMemo {
    pub(super) next: u32,
    pub(super) seen: HashMap<usize, u32>,
}

impl std::ops::Deref for PickleWriteMemo {
    type Target = u32;

    fn deref(&self) -> &Self::Target {
        &self.next
    }
}

impl std::ops::DerefMut for PickleWriteMemo {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.next
    }
}

pub(super) fn pickle_identity_key(obj: &PyObjectRef) -> Option<usize> {
    match &obj.payload {
        PyObjectPayload::List(_)
        | PyObjectPayload::Dict(_)
        | PyObjectPayload::MappingProxy(_)
        | PyObjectPayload::Instance(_) => Some(PyObjectRef::as_ptr(obj) as usize),
        _ => None,
    }
}

pub(super) fn p0_emit_get(buf: &mut Vec<u8>, id: u32) {
    buf.push(b'g');
    buf.extend_from_slice(format!("{}\n", id).as_bytes());
}

pub(super) fn p0_try_emit_get(
    obj: &PyObjectRef,
    buf: &mut Vec<u8>,
    memo: &PickleWriteMemo,
) -> bool {
    let Some(key) = pickle_identity_key(obj) else {
        return false;
    };
    let Some(id) = memo.seen.get(&key) else {
        return false;
    };
    p0_emit_get(buf, *id);
    true
}

pub(super) fn p0_emit_put_obj(obj: &PyObjectRef, buf: &mut Vec<u8>, memo: &mut PickleWriteMemo) {
    let id = **memo;
    **memo += 1;
    if let Some(key) = pickle_identity_key(obj) {
        memo.seen.insert(key, id);
    }
    buf.push(b'p');
    buf.extend_from_slice(format!("{}\n", id).as_bytes());
}

pub(super) fn p2_emit_get(buf: &mut Vec<u8>, id: u32) {
    if id <= 0xff {
        buf.push(b'h');
        buf.push(id as u8);
    } else {
        buf.push(b'j');
        buf.extend_from_slice(&id.to_le_bytes());
    }
}

pub(super) fn p2_try_emit_get(
    obj: &PyObjectRef,
    buf: &mut Vec<u8>,
    memo: &PickleWriteMemo,
) -> bool {
    let Some(key) = pickle_identity_key(obj) else {
        return false;
    };
    let Some(id) = memo.seen.get(&key) else {
        return false;
    };
    p2_emit_get(buf, *id);
    true
}

pub(super) fn p2_emit_put_obj(obj: &PyObjectRef, buf: &mut Vec<u8>, memo: &mut PickleWriteMemo) {
    let id = **memo;
    **memo += 1;
    if let Some(key) = pickle_identity_key(obj) {
        memo.seen.insert(key, id);
    }
    if id <= 0xff {
        buf.push(b'q');
        buf.push(id as u8);
    } else {
        buf.push(b'r');
        buf.extend_from_slice(&id.to_le_bytes());
    }
}

pub(super) fn p0_escape_unicode(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in s.chars() {
        match ch {
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\0' => out.extend_from_slice(b"\\x00"),
            c if c.is_ascii() => out.push(c as u8),
            c if (c as u32) <= 0xff => {
                out.extend_from_slice(format!("\\x{:02x}", c as u32).as_bytes());
            }
            c if (c as u32) <= 0xffff => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => {
                out.extend_from_slice(format!("\\U{:08x}", c as u32).as_bytes());
            }
        }
    }
    out
}

pub(super) fn p0_escape_bytes(b: &[u8]) -> Vec<u8> {
    let mut out = vec![b'\''];
    for &byte in b {
        match byte {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\'' => out.extend_from_slice(b"\\'"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x20..=0x7e => out.push(byte),
            _ => out.extend_from_slice(format!("\\x{:02x}", byte).as_bytes()),
        }
    }
    out.push(b'\'');
    out
}
