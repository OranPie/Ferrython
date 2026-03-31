//! Ferrython — A Rust implementation of the Python 3.8 interpreter.

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        // Interactive REPL mode
        println!("Ferrython 0.1.0 (Python 3.8 compatible)");
        println!("Type \"help\", \"copyright\", \"credits\" or \"license\" for more information.");
        // TODO: launch REPL
        return;
    }

    // Check for -c flag
    if args[1] == "-c" {
        if args.len() < 3 {
            eprintln!("Argument expected for the -c option");
            process::exit(2);
        }
        run_string(&args[2], "<string>");
        return;
    }

    // Check for -m flag
    if args[1] == "-m" {
        if args.len() < 3 {
            eprintln!("No module name specified");
            process::exit(2);
        }
        eprintln!("ferrython: -m flag not yet implemented");
        process::exit(1);
    }

    // Check for --version
    if args[1] == "--version" || args[1] == "-V" {
        println!("Ferrython 0.1.0 (Python 3.8 compatible)");
        return;
    }

    // Check for --dis flag (bytecode disassembly)
    if args[1] == "--dis" {
        if args.len() < 3 {
            eprintln!("Usage: ferrython --dis <script.py>");
            process::exit(2);
        }
        let filename = &args[2];
        match fs::read_to_string(filename) {
            Ok(source) => dis_string(&source, filename),
            Err(e) => {
                eprintln!("ferrython: can't open file '{}': {}", filename, e);
                process::exit(2);
            }
        }
        return;
    }

    // Run a script file
    let filename = &args[1];
    match fs::read_to_string(filename) {
        Ok(source) => run_string(&source, filename),
        Err(e) => {
            eprintln!("ferrython: can't open file '{}': {}", filename, e);
            process::exit(2);
        }
    }
}

fn run_string(source: &str, filename: &str) {
    // Parse
    let module = match ferrython_parser::parse(source, filename) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("  File \"{}\"", filename);
            eprintln!("SyntaxError: {}", e);
            process::exit(1);
        }
    };

    // Compile
    let code = match ferrython_compiler::compile(&module, filename) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("  File \"{}\", compilation error", filename);
            eprintln!("CompileError: {}", e);
            process::exit(1);
        }
    };

    // Execute
    let mut vm = ferrython_vm::VirtualMachine::new();
    match vm.execute(code) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Traceback (most recent call last):");
            eprintln!("  File \"{}\"", filename);
            eprintln!("{}: {}", e.kind, e.message);
            process::exit(1);
        }
    }
}

fn dis_string(source: &str, filename: &str) {
    use ferrython_bytecode::code::ConstantValue;

    let module = match ferrython_parser::parse(source, filename) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("SyntaxError: {}", e);
            process::exit(1);
        }
    };

    let code = match ferrython_compiler::compile(&module, filename) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("CompileError: {}", e);
            process::exit(1);
        }
    };

    dis_code(&code, 0);
}

fn dis_code(code: &ferrython_bytecode::code::CodeObject, indent: usize) {
    use ferrython_bytecode::code::ConstantValue;

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
            ferrython_bytecode::opcode::Opcode::LoadConst => {
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
            ferrython_bytecode::opcode::Opcode::LoadName
            | ferrython_bytecode::opcode::Opcode::StoreName
            | ferrython_bytecode::opcode::Opcode::LoadGlobal
            | ferrython_bytecode::opcode::Opcode::StoreGlobal
            | ferrython_bytecode::opcode::Opcode::LoadAttr
            | ferrython_bytecode::opcode::Opcode::StoreAttr => {
                if let Some(n) = code.names.get(instr.arg as usize) {
                    format!("({})", n)
                } else {
                    format!("(?{}?)", instr.arg)
                }
            }
            ferrython_bytecode::opcode::Opcode::LoadFast
            | ferrython_bytecode::opcode::Opcode::StoreFast => {
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
