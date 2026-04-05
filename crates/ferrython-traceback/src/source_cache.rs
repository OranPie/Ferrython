//! Source line cache — like CPython's `linecache` module.
//!
//! Caches file contents in memory so repeated traceback formatting doesn't
//! re-read the same source files. Thread-safe via `parking_lot::RwLock`.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::io::BufRead;
use std::sync::OnceLock;

/// Cached file contents: maps filename → lines (1-indexed via Vec index + 1).
static CACHE: OnceLock<RwLock<HashMap<String, Option<Vec<String>>>>> = OnceLock::new();

fn cache() -> &'static RwLock<HashMap<String, Option<Vec<String>>>> {
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// A thread-safe source line cache.
pub struct SourceCache;

impl SourceCache {
    /// Get a specific line from a source file (1-indexed).
    /// Returns `None` if the file can't be read or the line is out of range.
    pub fn get_line(filename: &str, lineno: u32) -> Option<String> {
        if lineno == 0 {
            return None;
        }
        let lines = Self::get_lines(filename)?;
        lines.get((lineno as usize).saturating_sub(1)).cloned()
    }

    /// Get all lines from a source file, loading and caching if needed.
    pub fn get_lines(filename: &str) -> Option<Vec<String>> {
        // Check cache first (read lock)
        {
            let cache_r = cache().read();
            if let Some(entry) = cache_r.get(filename) {
                return entry.clone();
            }
        }

        // Load from disk and cache (write lock)
        let lines = Self::load_file(filename);
        let mut cache_w = cache().write();
        cache_w.insert(filename.to_string(), lines.clone());
        lines
    }

    /// Clear the entire cache (like `linecache.clearcache()`).
    pub fn clear() {
        cache().write().clear();
    }

    /// Invalidate a specific file entry (like `linecache.checkcache()`).
    pub fn invalidate(filename: &str) {
        cache().write().remove(filename);
    }

    /// Load a file from disk into a vector of lines.
    fn load_file(filename: &str) -> Option<Vec<String>> {
        let file = std::fs::File::open(filename).ok()?;
        let reader = std::io::BufReader::new(file);
        let lines: Vec<String> = reader
            .lines()
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        Some(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_nonexistent_file() {
        assert!(SourceCache::get_line("/nonexistent/file.py", 1).is_none());
    }

    #[test]
    fn test_cache_zero_line() {
        assert!(SourceCache::get_line("anything.py", 0).is_none());
    }
}
