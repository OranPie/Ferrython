//! Traceback formatting and source line resolution.

use ferrython_bytecode::code::CodeObject;
use ferrython_core::error::PyException;

/// Resolve an instruction index to a source line number using the code object's
/// line number table. Returns `first_line_number` if no entry is found.
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

/// Format a Python-style traceback string from a `PyException`.
///
/// Shows source lines when files are readable. Example output:
/// ```text
/// Traceback (most recent call last):
///   File "test.py", line 5, in <module>
///     x = foo()
///   File "test.py", line 2, in foo
///     return 1 / 0
/// ZeroDivisionError: division by zero
/// ```
pub fn format_traceback(exc: &PyException) -> String {
    let mut out = String::new();
    // Print cause chain first (deepest cause printed first)
    if let Some(ref cause) = exc.cause {
        out.push_str(&format_traceback(cause));
        out.push('\n');
        out.push_str("\nThe above exception was the direct cause of the following exception:\n\n");
    } else if let Some(ref context) = exc.context {
        out.push_str(&format_traceback(context));
        out.push('\n');
        out.push_str("\nDuring handling of the above exception, another exception occurred:\n\n");
    }
    if !exc.traceback.is_empty() {
        out.push_str("Traceback (most recent call last):\n");
        for entry in &exc.traceback {
            out.push_str(&format!(
                "  File \"{}\", line {}, in {}\n",
                entry.filename, entry.lineno, entry.function,
            ));
            // Show the source line if available
            if let Some(line) = read_source_line(&entry.filename, entry.lineno) {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    out.push_str(&format!("    {}\n", trimmed));
                }
            }
        }
    }
    out.push_str(&format!("{}: {}", exc.kind, exc.message));
    out
}

/// Read a specific line from a source file. Returns None if the file can't be read.
fn read_source_line(filename: &str, lineno: u32) -> Option<String> {
    use std::io::BufRead;
    let file = std::fs::File::open(filename).ok()?;
    let reader = std::io::BufReader::new(file);
    reader.lines().nth((lineno as usize).saturating_sub(1))?.ok()
}
