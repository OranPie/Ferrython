#![allow(dead_code)]
//! Traceback formatting — CPython-compatible output with exception chaining.
//!
//! Produces output identical to CPython 3.8:
//! ```text
//! Traceback (most recent call last):
//!   File "test.py", line 5, in <module>
//!     x = foo()
//!   File "test.py", line 2, in foo
//!     return 1 / 0
//! ZeroDivisionError: division by zero
//! ```

use ferrython_bytecode::code::CodeObject;
use ferrython_core::error::{PyException, TracebackEntry};

use crate::source_cache::SourceCache;

/// Resolve an instruction index to a source line number using the code object's
/// line number table. Returns `first_line_number` if no matching entry is found.
pub fn resolve_lineno(code: &CodeObject, instruction_index: usize) -> u32 {
    let idx = instruction_index as u32;
    let mut lineno = code.first_line_number;
    for &(offset, line) in &code.line_number_table {
        if offset > idx {
            break;
        }
        lineno = line;
    }
    lineno
}

/// Format just the exception line: "ExceptionType: message"
pub fn format_exception_only(exc: &PyException) -> String {
    if exc.message.is_empty() {
        format!("{}", exc.kind)
    } else {
        format!("{}: {}", exc.kind, exc.message)
    }
}

/// Format a full CPython-style traceback string from a `PyException`.
///
/// Handles exception chaining:
/// - `raise X from Y` → "The above exception was the direct cause..."
/// - Implicit chaining → "During handling of the above exception..."
///
/// Uses the source cache for efficient repeated file reads.
pub fn format_traceback(exc: &PyException) -> String {
    let mut out = String::new();
    format_traceback_inner(exc, &mut out, 0);
    // Ensure trailing newline (CPython always ends with \n)
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Recursive traceback formatter with depth limit to prevent infinite loops
/// from circular exception chains.
fn format_traceback_inner(exc: &PyException, out: &mut String, depth: usize) {
    const MAX_CHAIN_DEPTH: usize = 32;
    if depth > MAX_CHAIN_DEPTH {
        out.push_str("... (exception chain too deep)\n");
        return;
    }

    // Print cause chain first (deepest cause printed first)
    if let Some(ref cause) = exc.cause {
        format_traceback_inner(cause, out, depth + 1);
        out.push('\n');
        out.push_str("\nThe above exception was the direct cause of the following exception:\n\n");
    } else if let Some(ref context) = exc.context {
        format_traceback_inner(context, out, depth + 1);
        out.push('\n');
        out.push_str("\nDuring handling of the above exception, another exception occurred:\n\n");
    }

    format_traceback_entries(&exc.traceback, out);
    out.push_str(&format_exception_only(exc));
}

/// Format a list of traceback entries with source line display.
fn format_traceback_entries(entries: &[TracebackEntry], out: &mut String) {
    if entries.is_empty() {
        return;
    }

    out.push_str("Traceback (most recent call last):\n");
    for entry in entries {
        out.push_str(&format!(
            "  File \"{}\", line {}, in {}\n",
            entry.filename, entry.lineno, entry.function,
        ));
        if let Some(line) = SourceCache::get_line(&entry.filename, entry.lineno) {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                out.push_str(&format!("    {}\n", trimmed));
            }
        }
    }
}

/// Format a single traceback entry as a string (for extract_tb output).
pub fn format_entry(entry: &TracebackEntry) -> String {
    let mut s = format!(
        "  File \"{}\", line {}, in {}\n",
        entry.filename, entry.lineno, entry.function,
    );
    if let Some(line) = SourceCache::get_line(&entry.filename, entry.lineno) {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            s.push_str(&format!("    {}\n", trimmed));
        }
    }
    s
}

/// Format a list of traceback entries as a list of strings (for format_tb).
pub fn format_tb(entries: &[TracebackEntry]) -> Vec<String> {
    entries.iter().map(|e| format_entry(e)).collect()
}

/// Format the full exception (traceback + exception line) as a list of strings.
/// Each string ends with a newline. This matches `traceback.format_exception()`.
pub fn format_exception_list(exc: &PyException) -> Vec<String> {
    let mut result = Vec::new();

    if !exc.traceback.is_empty() {
        result.push("Traceback (most recent call last):\n".to_string());
        for entry in &exc.traceback {
            result.push(format!(
                "  File \"{}\", line {}, in {}\n",
                entry.filename, entry.lineno, entry.function,
            ));
            if let Some(line) = SourceCache::get_line(&entry.filename, entry.lineno) {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    result.push(format!("    {}\n", trimmed));
                }
            }
        }
    }

    result.push(format!("{}\n", format_exception_only(exc)));
    result
}

/// Print a formatted traceback to stderr (like `traceback.print_exception()`).
pub fn print_exception(exc: &PyException) {
    eprint!("{}", format_traceback(exc));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrython_core::error::ExceptionKind;

    #[test]
    fn test_format_exception_only() {
        let exc = PyException::new(ExceptionKind::ValueError, "bad value");
        assert_eq!(format_exception_only(&exc), "ValueError: bad value");
    }

    #[test]
    fn test_format_exception_only_no_message() {
        let exc = PyException::new(ExceptionKind::StopIteration, "");
        assert_eq!(format_exception_only(&exc), "StopIteration");
    }

    #[test]
    fn test_format_traceback_no_entries() {
        let exc = PyException::new(ExceptionKind::TypeError, "wrong type");
        let formatted = format_traceback(&exc);
        assert_eq!(formatted, "TypeError: wrong type\n");
    }

    #[test]
    fn test_format_traceback_with_entries() {
        let mut exc = PyException::new(ExceptionKind::ZeroDivisionError, "division by zero");
        exc.traceback.push(TracebackEntry {
            filename: "<test>".to_string(),
            function: "<module>".to_string(),
            lineno: 1,
        });
        let formatted = format_traceback(&exc);
        assert!(formatted.contains("Traceback (most recent call last):"));
        assert!(formatted.contains("File \"<test>\", line 1, in <module>"));
        assert!(formatted.contains("ZeroDivisionError: division by zero"));
    }

    #[test]
    fn test_exception_chaining_cause() {
        let cause = PyException::new(ExceptionKind::ValueError, "bad");
        let mut exc = PyException::new(ExceptionKind::RuntimeError, "wrapped");
        exc.cause = Some(Box::new(cause));
        let formatted = format_traceback(&exc);
        assert!(formatted.contains("ValueError: bad"));
        assert!(formatted.contains("direct cause"));
        assert!(formatted.contains("RuntimeError: wrapped"));
    }
}
