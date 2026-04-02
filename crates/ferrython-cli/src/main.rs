//! Ferrython — A Rust implementation of the Python 3.8 interpreter.

use std::env;
use std::fs;
use std::io::{self, Write, BufRead};
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        // Interactive REPL mode
        ferrython_repl::run_repl();
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
    // Add script directory to import search paths
    if let Some(parent) = std::path::Path::new(filename).parent() {
        ferrython_import::prepend_search_path(parent.to_path_buf());
    }
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
            eprintln!("{}", ferrython_debug::format_traceback(&e));
            process::exit(1);
        }
    }
}

fn dis_string(source: &str, filename: &str) {
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

    ferrython_debug::dis_code(&code, 0);
}
