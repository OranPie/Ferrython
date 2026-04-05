//! Ferrython interactive REPL.
//!
//! Provides the `>>> ` prompt with multi-line input, persistent globals,
//! automatic expression result printing (via `PrintExpr`), and `_` for last result.

use std::io::{self, Write, BufRead};

/// Run the interactive REPL.
pub fn run_repl() {
    println!("Ferrython 0.1.0 (Python 3.8 compatible)");
    println!("Type \"exit()\" or \"quit()\" to exit.");

    let mut vm = ferrython_vm::VirtualMachine::new();
    let globals = ferrython_vm::VirtualMachine::new_globals();

    // Initialize _ = None in globals
    globals.write().insert(
        compact_str::CompactString::from("_"),
        ferrython_core::object::PyObject::none(),
    );

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    let mut pending_line: Option<String> = None;

    loop {
        let line = if let Some(pend) = pending_line.take() {
            print!(">>> ");
            io::stdout().flush().unwrap();
            pend
        } else {
            print!(">>> ");
            io::stdout().flush().unwrap();
            match lines.next() {
                Some(Ok(l)) => l,
                _ => break,
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        if trimmed == "exit()" || trimmed == "quit()" { break; }

        // Collect multi-line blocks (if line ends with ':' or starts with '@')
        let mut source = line.clone();
        let is_decorator_start = trimmed.starts_with('@');
        if trimmed.ends_with(':') || is_decorator_start
            || trimmed.ends_with('\\') || trimmed.ends_with(',')
            || trimmed.ends_with('(') || trimmed.ends_with('[') || trimmed.ends_with('{')
        {
            source.push('\n');
            let mut consecutive_blanks = 0u32;
            let mut after_decorator = is_decorator_start;
            loop {
                print!("... ");
                io::stdout().flush().unwrap();
                match lines.next() {
                    Some(Ok(cont)) => {
                        if cont.trim().is_empty() {
                            consecutive_blanks += 1;
                            if consecutive_blanks >= 2 {
                                source.push('\n');
                                break;
                            }
                            source.push('\n');
                            continue;
                        }
                        consecutive_blanks = 0;
                        let leading_spaces = cont.len() - cont.trim_start().len();
                        let cont_trimmed = cont.trim();
                        // Continuation keywords that belong to the current compound statement
                        let is_continuation = cont_trimmed.starts_with("except ")
                            || cont_trimmed.starts_with("except:")
                            || cont_trimmed.starts_with("finally:")
                            || cont_trimmed.starts_with("elif ")
                            || cont_trimmed.starts_with("else:")
                            || cont_trimmed == "else"
                            || cont_trimmed == "finally"
                            || cont_trimmed == "except"
                            || (after_decorator && (cont_trimmed.starts_with("def ") || cont_trimmed.starts_with("class ") || cont_trimmed.starts_with("@")));
                        if after_decorator && (cont_trimmed.starts_with("def ") || cont_trimmed.starts_with("class ")) {
                            after_decorator = false;
                        }
                        if leading_spaces == 0 && !cont_trimmed.is_empty() && !is_continuation {
                            // Dedented back to column 0 — end the block.
                            // Save this line for the next iteration.
                            source.push('\n');
                            pending_line = Some(cont);
                            break;
                        }
                        source.push_str(&cont);
                        source.push('\n');
                    }
                    _ => break,
                }
            }
        }

        // Parse and compile in interactive mode
        match ferrython_parser::parse(&source, "<stdin>") {
            Ok(module) => {
                match ferrython_compiler::compile_interactive(&module, "<stdin>") {
                    Ok(code) => {
                        match vm.execute_with_globals(std::sync::Arc::new(code), globals.clone()) {
                            Ok(_) => {}
                            Err(e) => eprintln!("{}", ferrython_debug::format_traceback(&e)),
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
