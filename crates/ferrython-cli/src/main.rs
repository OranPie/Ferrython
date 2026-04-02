//! Ferrython — A Rust implementation of the Python 3.8 interpreter.

use std::env;
use std::fs;
use std::process;

/// Unified error for the parse → compile → execute pipeline.
enum PipelineError {
    Parse(ferrython_parser::ParseError),
    Compile(ferrython_compiler::CompileError),
    Runtime(ferrython_core::error::PyException),
}

impl From<ferrython_parser::ParseError> for PipelineError {
    fn from(e: ferrython_parser::ParseError) -> Self { Self::Parse(e) }
}
impl From<ferrython_compiler::CompileError> for PipelineError {
    fn from(e: ferrython_compiler::CompileError) -> Self { Self::Compile(e) }
}
impl From<ferrython_core::error::PyException> for PipelineError {
    fn from(e: ferrython_core::error::PyException) -> Self { Self::Runtime(e) }
}

impl PipelineError {
    fn report(&self, filename: &str) {
        match self {
            Self::Parse(e) => {
                eprintln!("  File \"{}\"", filename);
                eprintln!("SyntaxError: {}", e);
            }
            Self::Compile(e) => {
                eprintln!("  File \"{}\", compilation error", filename);
                eprintln!("CompileError: {}", e);
            }
            Self::Runtime(e) => {
                eprintln!("{}", ferrython_debug::format_traceback(e));
            }
        }
    }
}

fn main() {
    // Initialize GC cycle detection
    ferrython_core::object::init_gc();
    
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        ferrython_repl::run_repl();
        return;
    }

    if args[1] == "-c" {
        if args.len() < 3 {
            eprintln!("Argument expected for the -c option");
            process::exit(2);
        }
        run_string(&args[2], "<string>");
        return;
    }

    if args[1] == "-m" {
        if args.len() < 3 {
            eprintln!("No module name specified");
            process::exit(2);
        }
        eprintln!("ferrython: -m flag not yet implemented");
        process::exit(1);
    }

    if args[1] == "--version" || args[1] == "-V" {
        println!("Ferrython 0.1.0 (Python 3.8 compatible)");
        return;
    }

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

    let filename = &args[1];
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

fn execute_pipeline(source: &str, filename: &str) -> Result<(), PipelineError> {
    let module = ferrython_parser::parse(source, filename)?;
    let code = ferrython_compiler::compile(&module, filename)?;
    let mut vm = ferrython_vm::VirtualMachine::new();
    vm.execute(code)?;
    Ok(())
}

fn run_string(source: &str, filename: &str) {
    if let Err(e) = execute_pipeline(source, filename) {
        e.report(filename);
        process::exit(1);
    }
}

fn dis_pipeline(source: &str, filename: &str) -> Result<(), PipelineError> {
    let module = ferrython_parser::parse(source, filename)?;
    let code = ferrython_compiler::compile(&module, filename)?;
    ferrython_debug::dis_code(&code, 0);
    Ok(())
}

fn dis_string(source: &str, filename: &str) {
    if let Err(e) = dis_pipeline(source, filename) {
        e.report(filename);
        process::exit(1);
    }
}
