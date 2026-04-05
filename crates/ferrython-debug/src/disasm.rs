//! Bytecode disassembly — human-readable display of CodeObject instructions.

use ferrython_bytecode::code::{CodeObject, ConstantValue};
use ferrython_bytecode::opcode::Opcode;

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
    println!("{}  flags: {:?}", pad, code.flags);
    println!("{}  arg_count: {}, kwonly: {}, posonly: {}",
        pad, code.arg_count, code.kwonlyarg_count, code.posonlyarg_count);
    println!("{}  consts: {} items", pad, code.constants.len());
    for (i, c) in code.constants.iter().enumerate() {
        let desc = match c {
            ConstantValue::None => "None".to_string(),
            ConstantValue::Bool(b) => format!("{}", b),
            ConstantValue::Integer(n) => format!("{}", n),
            ConstantValue::BigInteger(n) => format!("{}n", n),
            ConstantValue::Float(f) => format!("{}", f),
            ConstantValue::Complex { real, imag } => format!("{}+{}j", real, imag),
            ConstantValue::Str(s) => {
                if s.len() > 40 { format!("'{:.37}...'", s) }
                else { format!("'{}'", s) }
            }
            ConstantValue::Bytes(b) => format!("b'...' ({} bytes)", b.len()),
            ConstantValue::Ellipsis => "...".to_string(),
            ConstantValue::Code(c) => format!("<code {}>", c.name),
            ConstantValue::Tuple(t) => format!("tuple({} items)", t.len()),
            ConstantValue::FrozenSet(s) => format!("frozenset({} items)", s.len()),
        };
        println!("{}    [{}] {}", pad, i, desc);
    }
    println!();

    // Print instructions with line number annotations
    let mut last_lineno = 0u32;
    for (i, instr) in code.instructions.iter().enumerate() {
        let lineno = super::traceback::resolve_lineno(code, i);
        let line_marker = if lineno != last_lineno {
            last_lineno = lineno;
            format!("{:>4}", lineno)
        } else {
            "    ".to_string()
        };

        let arg_desc = format_arg_desc(code, instr.op, instr.arg);
        println!("{}{} {:4} {:24} {}", pad, line_marker, i, format!("{:?}", instr.op), arg_desc);
    }

    // Recurse into nested code objects
    for c in &code.constants {
        if let ConstantValue::Code(nested) = c {
            println!();
            dis_code(nested, indent + 2);
        }
    }
}

/// Disassemble a `CodeObject` to stderr, recursing into nested code objects.
pub fn dis_code_stderr(code: &CodeObject, indent: usize) {
    let pad = " ".repeat(indent);
    eprintln!("{}=== Code: {} ===", pad, code.name);
    eprintln!("{}  names: {:?}", pad, code.names);
    eprintln!("{}  varnames: {:?}", pad, code.varnames);
    if !code.cellvars.is_empty() {
        eprintln!("{}  cellvars: {:?}", pad, code.cellvars);
    }
    if !code.freevars.is_empty() {
        eprintln!("{}  freevars: {:?}", pad, code.freevars);
    }
    eprintln!("{}  flags: {:?}", pad, code.flags);
    eprintln!("{}  arg_count: {}, kwonly: {}, posonly: {}",
        pad, code.arg_count, code.kwonlyarg_count, code.posonlyarg_count);
    eprintln!("{}  consts: {} items", pad, code.constants.len());
    for (i, c) in code.constants.iter().enumerate() {
        let desc = match c {
            ConstantValue::None => "None".to_string(),
            ConstantValue::Bool(b) => format!("{}", b),
            ConstantValue::Integer(n) => format!("{}", n),
            ConstantValue::BigInteger(n) => format!("{}n", n),
            ConstantValue::Float(f) => format!("{}", f),
            ConstantValue::Complex { real, imag } => format!("{}+{}j", real, imag),
            ConstantValue::Str(s) => {
                if s.len() > 40 { format!("'{:.37}...'", s) }
                else { format!("'{}'", s) }
            }
            ConstantValue::Bytes(b) => format!("b'...' ({} bytes)", b.len()),
            ConstantValue::Ellipsis => "...".to_string(),
            ConstantValue::Code(c) => format!("<code {}>", c.name),
            ConstantValue::Tuple(t) => format!("tuple({} items)", t.len()),
            ConstantValue::FrozenSet(s) => format!("frozenset({} items)", s.len()),
        };
        eprintln!("{}    [{}] {}", pad, i, desc);
    }
    eprintln!();

    let mut last_lineno = 0u32;
    for (i, instr) in code.instructions.iter().enumerate() {
        let lineno = super::traceback::resolve_lineno(code, i);
        let line_marker = if lineno != last_lineno {
            last_lineno = lineno;
            format!("{:>4}", lineno)
        } else {
            "    ".to_string()
        };

        let arg_desc = format_arg_desc(code, instr.op, instr.arg);
        eprintln!("{}{} {:4} {:24} {}", pad, line_marker, i, format!("{:?}", instr.op), arg_desc);
    }

    for c in &code.constants {
        if let ConstantValue::Code(nested) = c {
            eprintln!();
            dis_code_stderr(nested, indent + 2);
        }
    }
}

/// Format the argument description for a single instruction.
fn format_arg_desc(code: &CodeObject, op: Opcode, arg: u32) -> String {
    match op {
        Opcode::LoadConst => {
            if let Some(c) = code.constants.get(arg as usize) {
                match c {
                    ConstantValue::Str(s) => {
                        if s.len() > 30 { format!("('{:.27}...')", s) }
                        else { format!("('{}')", s) }
                    }
                    ConstantValue::Integer(n) => format!("({})", n),
                    ConstantValue::Float(f) => format!("({})", f),
                    ConstantValue::None => "(None)".to_string(),
                    ConstantValue::Bool(b) => format!("({})", b),
                    ConstantValue::Code(c) => format!("(<code {}>)", c.name),
                    ConstantValue::Tuple(t) => format!("(tuple/{})", t.len()),
                    ConstantValue::Ellipsis => "(...)".to_string(),
                    _ => format!("(const[{}])", arg),
                }
            } else {
                format!("(?{}?)", arg)
            }
        }
        Opcode::LoadName | Opcode::StoreName | Opcode::DeleteName
        | Opcode::LoadGlobal | Opcode::StoreGlobal | Opcode::DeleteGlobal
        | Opcode::LoadAttr | Opcode::StoreAttr | Opcode::DeleteAttr
        | Opcode::ImportName | Opcode::ImportFrom => {
            if let Some(n) = code.names.get(arg as usize) {
                format!("({})", n)
            } else {
                format!("(?name{}?)", arg)
            }
        }
        Opcode::LoadFast | Opcode::StoreFast | Opcode::DeleteFast => {
            if let Some(n) = code.varnames.get(arg as usize) {
                format!("({})", n)
            } else {
                format!("(?var{}?)", arg)
            }
        }
        Opcode::LoadDeref | Opcode::StoreDeref | Opcode::LoadClosure => {
            let nc = code.cellvars.len();
            let idx = arg as usize;
            if idx < nc {
                if let Some(n) = code.cellvars.get(idx) {
                    format!("(cell: {})", n)
                } else {
                    format!("(?cell{}?)", arg)
                }
            } else if let Some(n) = code.freevars.get(idx - nc) {
                format!("(free: {})", n)
            } else {
                format!("(?deref{}?)", arg)
            }
        }
        Opcode::CompareOp => {
            let op_name = match arg {
                0 => "<", 1 => "<=", 2 => "==", 3 => "!=", 4 => ">", 5 => ">=",
                6 => "in", 7 => "not in", 8 => "is", 9 => "is not",
                10 => "exception match",
                _ => "?",
            };
            format!("({})", op_name)
        }
        Opcode::JumpAbsolute | Opcode::JumpForward
        | Opcode::PopJumpIfTrue | Opcode::PopJumpIfFalse
        | Opcode::JumpIfTrueOrPop | Opcode::JumpIfFalseOrPop
        | Opcode::SetupExcept | Opcode::SetupFinally
        | Opcode::ForIter => {
            format!("(to {})", arg)
        }
        _ => {
            if arg != 0 { format!("{}", arg) }
            else { String::new() }
        }
    }
}
