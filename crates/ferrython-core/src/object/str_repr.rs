//! Compact inline/heap string representation for Python string payloads.

use compact_str::CompactString;
use std::fmt;

const INLINE_STR_MAX: usize = 15;
const HEAP_SENTINEL: u8 = 0xFF;

/// Compact string representation: 16 bytes.
/// Inline: tag = length (0..=15), data[0..len] = UTF-8 bytes.
/// Heap:   tag = 0xFF, ptr = Box<CompactString> (at offset 8, 8-byte aligned).
#[repr(C)]
pub struct StrRepr {
    tag: u8,
    inline_data: [u8; 7],
    rest: u64,
}

const _STR_REPR_SIZE_CHECK: () = assert!(std::mem::size_of::<StrRepr>() == 16);
const _STR_REPR_ALIGN_CHECK: () = assert!(std::mem::align_of::<StrRepr>() == 8);

impl StrRepr {
    /// Create from a byte slice (must be valid UTF-8).
    #[inline(always)]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let len = bytes.len();
        if len <= INLINE_STR_MAX {
            Self::from_inline_bytes(bytes)
        } else {
            Self::from_compact(CompactString::from(unsafe {
                std::str::from_utf8_unchecked(bytes)
            }))
        }
    }

    /// Create from a CompactString (may inline if short enough).
    #[inline(always)]
    pub fn from_compact(s: CompactString) -> Self {
        if s.len() <= INLINE_STR_MAX {
            Self::from_inline_bytes(s.as_bytes())
        } else {
            let b = super::constructors::alloc_str_box(s);
            Self::from_box(b)
        }
    }

    /// Create from a pre-allocated Box<CompactString> (always heap).
    #[inline(always)]
    pub fn from_box(b: Box<CompactString>) -> Self {
        let ptr = Box::into_raw(b);
        StrRepr {
            tag: HEAP_SENTINEL,
            inline_data: [0; 7],
            rest: ptr as u64,
        }
    }

    #[inline(always)]
    pub fn is_inline(&self) -> bool {
        self.tag != HEAP_SENTINEL
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        if self.is_inline() {
            self.tag as usize
        } else {
            unsafe { &*self.heap_ptr() }.len()
        }
    }

    #[inline(always)]
    pub fn as_str(&self) -> &str {
        if self.is_inline() {
            let len = self.tag as usize;
            unsafe {
                let base = &self.tag as *const u8;
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(base.add(1), len))
            }
        } else {
            unsafe { &*self.heap_ptr() }.as_str()
        }
    }

    /// Convert to CompactString (clones for inline, clones inner for heap).
    #[inline]
    pub fn to_compact_string(&self) -> CompactString {
        if self.is_inline() {
            CompactString::from(self.as_str())
        } else {
            unsafe { &*self.heap_ptr() }.clone()
        }
    }

    /// In-place append. Converts inline to heap if needed.
    #[inline]
    pub fn push_str(&mut self, suffix: &str) {
        if self.is_inline() {
            let cur_len = self.tag as usize;
            let new_len = cur_len + suffix.len();
            if new_len <= INLINE_STR_MAX {
                unsafe {
                    let base = &self.tag as *const u8 as *mut u8;
                    std::ptr::copy_nonoverlapping(
                        suffix.as_ptr(),
                        base.add(1 + cur_len),
                        suffix.len(),
                    );
                }
                self.tag = new_len as u8;
            } else {
                let mut cs = CompactString::with_capacity(new_len);
                cs.push_str(self.as_str());
                cs.push_str(suffix);
                self.tag = HEAP_SENTINEL;
                self.inline_data = [0u8; 7];
                self.rest = Box::into_raw(Box::new(cs)) as u64;
            }
        } else {
            unsafe { &mut *self.heap_ptr() }.push_str(suffix);
        }
    }

    #[inline(always)]
    fn from_inline_bytes(bytes: &[u8]) -> Self {
        let len = bytes.len();
        let mut inline_data = [0u8; 7];
        let mut rest_bytes = [0u8; 8];
        if len <= 7 {
            inline_data[..len].copy_from_slice(bytes);
        } else {
            inline_data.copy_from_slice(&bytes[..7]);
            rest_bytes[..len - 7].copy_from_slice(&bytes[7..]);
        }
        StrRepr {
            tag: len as u8,
            inline_data,
            rest: u64::from_ne_bytes(rest_bytes),
        }
    }

    #[inline(always)]
    unsafe fn heap_ptr(&self) -> *mut CompactString {
        self.rest as *mut CompactString
    }
}

impl Clone for StrRepr {
    #[inline(always)]
    fn clone(&self) -> Self {
        if self.is_inline() {
            StrRepr {
                tag: self.tag,
                inline_data: self.inline_data,
                rest: self.rest,
            }
        } else {
            let s = unsafe { &*self.heap_ptr() };
            StrRepr::from_compact(s.clone())
        }
    }
}

impl Drop for StrRepr {
    #[inline(always)]
    fn drop(&mut self) {
        if !self.is_inline() {
            let b = unsafe { Box::from_raw(self.heap_ptr()) };
            super::constructors::recycle_str_box(b);
        }
    }
}

impl fmt::Debug for StrRepr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StrRepr({:?})", self.as_str())
    }
}

impl std::hash::Hash for StrRepr {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialEq for StrRepr {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for StrRepr {}

impl PartialOrd for StrRepr {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.as_str().cmp(other.as_str()))
    }
}

impl Ord for StrRepr {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl std::ops::Deref for StrRepr {
    type Target = str;

    #[inline(always)]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for StrRepr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
