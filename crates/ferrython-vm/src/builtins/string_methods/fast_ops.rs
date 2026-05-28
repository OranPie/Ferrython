//! Fast string split/search/replace/join helpers.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

/// Fast whitespace split — single byte-scan pass, no iterator/filter overhead.
/// Matches Python semantics: strips leading/trailing whitespace, splits on runs of whitespace.
#[allow(dead_code)]
#[inline]
pub(super) fn split_whitespace_fast(bytes: &[u8], maxsplit: Option<usize>) -> Vec<PyObjectRef> {
    let len = bytes.len();
    if len == 0 {
        return Vec::new();
    }
    let max = maxsplit.unwrap_or(usize::MAX);
    let mut parts = Vec::with_capacity(12); // CPython's PREALLOC_SIZE
    let mut splits = 0usize;
    let mut i = 0;
    while i < len {
        // Skip whitespace
        let b = unsafe { *bytes.get_unchecked(i) };
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == 0x0b || b == 0x0c {
            i += 1;
            continue;
        }
        if splits >= max {
            parts.push(PyObject::str_from_utf8_slice(&bytes[i..]));
            return parts;
        }
        // Find end of word
        let start = i;
        i += 1;
        while i < len {
            let b = unsafe { *bytes.get_unchecked(i) };
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == 0x0b || b == 0x0c {
                break;
            }
            i += 1;
        }
        parts.push(PyObject::str_from_utf8_slice(&bytes[start..i]));
        splits += 1;
    }
    parts
}

/// Fast single-byte separator split — single pass, no pre-count.
/// Uses PREALLOC=12 (matches CPython's PREALLOC_SIZE) to avoid reallocation for typical cases.
#[allow(dead_code)]
#[inline]
pub(super) fn split_single_byte(bytes: &[u8], sep: u8) -> Vec<PyObjectRef> {
    let len = bytes.len();
    let mut parts = Vec::with_capacity(12);
    let mut start = 0;
    // Direct byte scan for short strings (avoids memchr function call overhead)
    if len <= 256 {
        for i in 0..len {
            if unsafe { *bytes.get_unchecked(i) } == sep {
                parts.push(PyObject::str_from_utf8_slice(&bytes[start..i]));
                start = i + 1;
            }
        }
    } else {
        for pos in memchr::memchr_iter(sep, bytes) {
            parts.push(PyObject::str_from_utf8_slice(&bytes[start..pos]));
            start = pos + 1;
        }
    }
    parts.push(PyObject::str_from_utf8_slice(&bytes[start..]));
    parts
}

/// Fast whitespace split that pushes directly into an existing Vec.
/// Avoids creating an intermediate Vec allocation.
#[inline]
pub(super) fn split_whitespace_into(
    bytes: &[u8],
    maxsplit: Option<usize>,
    parts: &mut Vec<PyObjectRef>,
) {
    let len = bytes.len();
    if len == 0 {
        return;
    }
    let max = maxsplit.unwrap_or(usize::MAX);
    let mut splits = 0usize;
    let mut i = 0;
    while i < len {
        let b = unsafe { *bytes.get_unchecked(i) };
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == 0x0b || b == 0x0c {
            i += 1;
            continue;
        }
        if splits >= max {
            parts.push(PyObject::str_from_utf8_slice(&bytes[i..]));
            return;
        }
        let start = i;
        i += 1;
        while i < len {
            let b = unsafe { *bytes.get_unchecked(i) };
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == 0x0b || b == 0x0c {
                break;
            }
            i += 1;
        }
        parts.push(PyObject::str_from_utf8_slice(&bytes[start..i]));
        splits += 1;
    }
}

/// Fast single-byte separator split that pushes directly into an existing Vec.
#[inline]
pub(super) fn split_single_byte_into(bytes: &[u8], sep: u8, parts: &mut Vec<PyObjectRef>) {
    let len = bytes.len();
    let mut start = 0;
    if len <= 256 {
        for i in 0..len {
            if unsafe { *bytes.get_unchecked(i) } == sep {
                parts.push(PyObject::str_from_utf8_slice(&bytes[start..i]));
                start = i + 1;
            }
        }
    } else {
        for pos in memchr::memchr_iter(sep, bytes) {
            parts.push(PyObject::str_from_utf8_slice(&bytes[start..pos]));
            start = pos + 1;
        }
    }
    parts.push(PyObject::str_from_utf8_slice(&bytes[start..]));
}

/// Replace occurrences of `old` with `new` in `s`, writing directly into a CompactString.
/// memchr-accelerated substring search: find `needle` in `haystack[start..]`.
/// Returns byte offset relative to `haystack` start.
/// For single-byte needles, uses memchr directly.
/// For multi-byte, uses memchr on first byte + memcmp verification.
#[inline]
pub(super) fn fast_find(haystack: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    let nlen = needle.len();
    if nlen == 0 {
        return Some(start);
    }
    let hay = &haystack[start..];
    if hay.len() < nlen {
        return None;
    }
    let first = needle[0];
    if nlen == 1 {
        return memchr::memchr(first, hay).map(|i| i + start);
    }
    let mut offset = 0;
    while offset + nlen <= hay.len() {
        match memchr::memchr(first, &hay[offset..]) {
            None => return None,
            Some(i) => {
                let pos = offset + i;
                if pos + nlen > hay.len() {
                    return None;
                }
                if &hay[pos..pos + nlen] == needle {
                    return Some(pos + start);
                }
                offset = pos + 1;
            }
        }
    }
    None
}

/// Count occurrences of `needle` in `haystack` (non-overlapping), up to `limit`.
#[inline]
pub(super) fn fast_count(haystack: &[u8], needle: &[u8], limit: usize) -> usize {
    let nlen = needle.len();
    if nlen == 0 || limit == 0 {
        return 0;
    }
    let mut count = 0usize;
    let mut start = 0usize;
    while count < limit {
        match fast_find(haystack, start, needle) {
            Some(pos) => {
                count += 1;
                start = pos + nlen;
            }
            None => break,
        }
    }
    count
}

/// Avoids the intermediate String allocation that `str::replace()` creates.
/// Uses memchr-accelerated search and exact-size pre-allocation.
pub(super) fn replace_into_compact(
    s: &str,
    old: &str,
    new: &str,
    max_count: Option<usize>,
) -> CompactString {
    if old.is_empty() {
        let char_count = s.chars().count();
        let limit = max_count.unwrap_or(char_count + 1);
        let actual = limit.min(char_count + 1);
        let mut result = CompactString::with_capacity(s.len() + new.len() * actual);
        let mut count = 0;
        for ch in s.chars() {
            if count < limit {
                result.push_str(new);
                count += 1;
            }
            result.push(ch);
        }
        if count < limit {
            result.push_str(new);
        }
        return result;
    }

    let sb = s.as_bytes();
    let old_b = old.as_bytes();
    let new_b = new.as_bytes();
    let old_len = old_b.len();
    let new_len = new_b.len();
    let limit = max_count.unwrap_or(usize::MAX);

    // Pre-count occurrences (memchr-accelerated)
    let occ = fast_count(sb, old_b, limit);
    if occ == 0 {
        return CompactString::from(s);
    }

    // Same-length: in-place overwrite (no realloc)
    if old_len == new_len {
        let mut bytes = Vec::from(sb);
        let mut start = 0usize;
        for _ in 0..occ {
            if let Some(pos) = fast_find(sb, start, old_b) {
                bytes[pos..pos + old_len].copy_from_slice(new_b);
                start = pos + old_len;
            }
        }
        return unsafe { CompactString::from(String::from_utf8_unchecked(bytes)) };
    }

    // Exact-size allocation: for small results, use stack buffer to avoid Vec heap alloc
    let result_len = s.len() + occ * new_len - occ * old_len;

    // Stack buffer path: avoids Vec heap allocation for results ≤128 bytes
    if result_len <= 128 {
        let mut stack_buf = [0u8; 128];
        let out_ptr = stack_buf.as_mut_ptr();
        let mut src_pos = 0usize;
        let mut dst = 0usize;
        unsafe {
            for _ in 0..occ {
                if let Some(pos) = fast_find(sb, src_pos, old_b) {
                    let prefix_len = pos - src_pos;
                    if prefix_len > 0 {
                        std::ptr::copy_nonoverlapping(
                            sb.as_ptr().add(src_pos),
                            out_ptr.add(dst),
                            prefix_len,
                        );
                        dst += prefix_len;
                    }
                    if new_len > 0 {
                        std::ptr::copy_nonoverlapping(new_b.as_ptr(), out_ptr.add(dst), new_len);
                        dst += new_len;
                    }
                    src_pos = pos + old_len;
                }
            }
            let rem = sb.len() - src_pos;
            if rem > 0 {
                std::ptr::copy_nonoverlapping(sb.as_ptr().add(src_pos), out_ptr.add(dst), rem);
                dst += rem;
            }
            return CompactString::from(std::str::from_utf8_unchecked(&stack_buf[..dst]));
        }
    }

    let mut out = Vec::with_capacity(result_len);
    let mut src_pos = 0usize;
    let out_ptr: *mut u8 = out.as_mut_ptr();
    let mut dst = 0usize;

    unsafe {
        for _ in 0..occ {
            if let Some(pos) = fast_find(sb, src_pos, old_b) {
                let prefix_len = pos - src_pos;
                if prefix_len > 0 {
                    std::ptr::copy_nonoverlapping(
                        sb.as_ptr().add(src_pos),
                        out_ptr.add(dst),
                        prefix_len,
                    );
                    dst += prefix_len;
                }
                if new_len > 0 {
                    std::ptr::copy_nonoverlapping(new_b.as_ptr(), out_ptr.add(dst), new_len);
                    dst += new_len;
                }
                src_pos = pos + old_len;
            }
        }
        // Copy remainder
        let rem = sb.len() - src_pos;
        if rem > 0 {
            std::ptr::copy_nonoverlapping(sb.as_ptr().add(src_pos), out_ptr.add(dst), rem);
            dst += rem;
        }
        out.set_len(dst);
        CompactString::from(String::from_utf8_unchecked(out))
    }
}
/// Avoids cloning the list/tuple just to iterate.
pub(super) fn join_str_slice(sep: &str, items: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if items.is_empty() {
        return Ok(PyObject::str_val(CompactString::new("")));
    }
    if items.len() == 1 {
        return match items[0].as_str() {
            Some(s) => Ok(PyObject::str_val(CompactString::from(s))),
            None => Err(PyException::type_error(format!(
                "sequence item 0: expected str instance, {} found",
                items[0].type_name()
            ))),
        };
    }
    // Pre-compute total length, then build result with unsafe memcpy (skip bounds checks).
    let sep_len = sep.len();
    let sep_total = sep_len * (items.len() - 1);
    let mut total_len = sep_total;
    for (i, item) in items.iter().enumerate() {
        if let PyObjectPayload::Str(s) = &item.payload {
            total_len += s.len();
        } else {
            return Err(PyException::type_error(format!(
                "sequence item {}: expected str instance, {} found",
                i,
                item.type_name()
            )));
        }
    }
    // For small results, use a stack buffer to avoid heap allocation entirely.
    // CompactString stores up to 24 bytes inline; stack buffer avoids Vec alloc + copy.
    let sep_bytes = sep.as_bytes();
    if total_len <= 128 {
        let mut stack_buf = [0u8; 128];
        let base = stack_buf.as_mut_ptr();
        let mut offset = 0usize;
        for (i, item) in items.iter().enumerate() {
            if let PyObjectPayload::Str(s) = &item.payload {
                if i > 0 && sep_len > 0 {
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            sep_bytes.as_ptr(),
                            base.add(offset),
                            sep_len,
                        );
                    }
                    offset += sep_len;
                }
                let s_bytes = s.as_bytes();
                let s_len = s_bytes.len();
                if s_len > 0 {
                    unsafe {
                        std::ptr::copy_nonoverlapping(s_bytes.as_ptr(), base.add(offset), s_len);
                    }
                    offset += s_len;
                }
            }
        }
        return Ok(PyObject::str_from_utf8_slice(&stack_buf[..offset]));
    }
    // Large result: heap-allocated buffer
    let mut buf = Vec::<u8>::with_capacity(total_len);
    let base = buf.as_mut_ptr();
    let mut offset = 0usize;
    for (i, item) in items.iter().enumerate() {
        if let PyObjectPayload::Str(s) = &item.payload {
            if i > 0 && sep_len > 0 {
                unsafe {
                    std::ptr::copy_nonoverlapping(sep_bytes.as_ptr(), base.add(offset), sep_len);
                }
                offset += sep_len;
            }
            let s_bytes = s.as_bytes();
            let s_len = s_bytes.len();
            if s_len > 0 {
                unsafe {
                    std::ptr::copy_nonoverlapping(s_bytes.as_ptr(), base.add(offset), s_len);
                }
                offset += s_len;
            }
        }
    }
    unsafe {
        buf.set_len(offset);
    }
    // SAFETY: all input strings were valid UTF-8, separator is valid UTF-8
    let result_str = unsafe { String::from_utf8_unchecked(buf) };
    Ok(PyObject::str_val(CompactString::from(result_str)))
}
