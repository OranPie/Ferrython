//! Ferrython interactive REPL.
//!
//! Provides the `>>> ` prompt with multi-line input, persistent globals,
//! automatic expression result printing, and `_` for last result.

use std::io::{self, Write, BufRead};

/// Run the interactive REPL.
pub fn run_repl() {
    println!("Ferrython 0.1.0 (Python 3.8 compatible)");
    println!("Type \"exit()\" or \"quit()\" to exit.");

    let mut vm = ferrython_vm::VirtualMachine::new();
    let globals = ferrython_vm::VirtualMachine::new_globals();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        print!(">>> ");
        io::stdout().flush().unwrap();

        let line = match lines.next() {
            Some(Ok(l)) => l,
            _ => break,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        if trimmed == "exit()" || trimmed == "quit()" { break; }

        // Collect multi-line blocks (if line ends with ':' or starts with '@')
        let mut source = line.clone();
        if trimmed.ends_with(':') || trimmed.starts_with('@') {
            source.push('\n');
            loop {
                print!("... ");
                io::stdout().flush().unwrap();
                match lines.next() {
                    Some(Ok(cont)) => {
                        if cont.trim().is_empty() {
                            source.push('\n');
                            break;
                        }
                        source.push_str(&cont);
                        source.push('\n');
                    }
                    _ => break,
                }
            }
        }

        // Parse and execute
        match ferrython_parser::parse(&source, "<stdin>") {
            Ok(module) => {
                match ferrython_compiler::compile(&module, "<stdin>") {
                    Ok(code) => {
                        match vm.execute_with_globals(code, globals.clone()) {
                            Ok(_) => {}
                            Err(e) => eprintln!("{}: {}", e.kind, e.message),
                        }
                    }
                    Err(e) => eprintln!("CompileError: {}", e),
                }
            }
            Err(e) => eprintln!("SyntaxError: {}", e),
        }
    }
    println!();
}