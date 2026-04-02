//! Ferrython developer tools — bytecode disassembly, line number resolution,
//! and traceback formatting.

use ferrython_bytecode::code::{CodeObject, ConstantValue};
use ferrython_bytecode::opcode::Opcode;
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
/// Example output:
/// ```text
/// Traceback (most recent call last):
///   File "test.py", line 5, in <module>
///   File "test.py", line 2, in foo
/// TypeError: unsupported operand
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
        }
    }
    out.push_str(&format!("{}: {}", exc.kind, exc.message));
    out
}

/// Disassemble a `CodeObject` to stdout, recursing into nested code objects.
pub fn dis_code(code: &CodeObject, indent: usize) {
    let pad = " ".repeat(indent);
    println!("{}=== Code: {} ===", pad, code.name);
    println!("{}  names: {:?}", pad, code.names);
    println!("{}  varnames: {:?}", pad, code.varnames);
    if !code.cellvars.is_empty() {
        println!("{}  cellvars: {:?}", pad, code.cellvars);
    }
    if !code.freevars.is_empty() {
        println!("{}  freevars: {:?}", pad, code.freevars);
    }
    println!("{}  consts: {} items", pad, code.constants.len());
    for (i, c) in code.constants.iter().enumerate() {
        let desc = match c {
            ConstantValue::None => "None".to_string(),
            ConstantValue::Bool(b) => format!("{}", b),
            ConstantValue::Integer(n) => format!("{}", n),
            ConstantValue::Float(f) => format!("{}", f),
            ConstantValue::Str(s) => format!("'{}'", s),
            ConstantValue::Code(_) => "<code>".to_string(),
            ConstantValue::Tuple(t) => format!("tuple({})", t.len()),
            _ => "...".to_string(),
        };
        println!("{}    [{}] {}", pad, i, desc);
    }
    println!();
    for (i, instr) in code.instructions.iter().enumerate() {
        let arg_desc = match instr.op {
            Opcode::LoadConst => {
                if let Some(c) = code.constants.get(instr.arg as usize) {
                    match c {
                        ConstantValue::Str(s) => format!("('{}')", s),
                        ConstantValue::Integer(n) => format!("({})", n),
                        ConstantValue::None => "(None)".to_string(),
                        ConstantValue::Bool(b) => format!("({})", b),
                        ConstantValue::Code(c) => format!("(<code {}>)", c.name),
                        _ => format!("(const[{}])", instr.arg),
                    }
                } else {
                    format!("(?{}?)", instr.arg)
                }
            }
            Opcode::LoadName | Opcode::StoreName | Opcode::LoadGlobal
            | Opcode::StoreGlobal | Opcode::LoadAttr | Opcode::StoreAttr => {
                if let Some(n) = code.names.get(instr.arg as usize) {
                    format!("({})", n)
                } else {
                    format!("(?{}?)", instr.arg)
                }
            }
            Opcode::LoadFast | Opcode::StoreFast => {
                if let Some(n) = code.varnames.get(instr.arg as usize) {
                    format!("({})", n)
                } else {
                    format!("(?{}?)", instr.arg)
                }
            }
            _ => {
                if instr.arg != 0 {
                    format!("{}", instr.arg)
                } else {
                    String::new()
                }
            }
        };
        println!("{}{:4} {:?} {}", pad, i, instr.op, arg_desc);
    }

    // Recurse into nested code objects
    for c in &code.constants {
        if let ConstantValue::Code(nested) = c {
            println!();
            dis_code(nested, indent + 2);
        }
    }
}