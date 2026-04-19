//! CPython regression test suite — integration tests for Ferrython.
//!
//! Each test below runs a vendored CPython 3.8 test file through the full
//! Ferrython pipeline (parse → compile → VM) via a subprocess and checks
//! that it exits successfully.
//!
//! Run the whole suite:
//!
//! ```
//! cargo test -p ferrython-cli --test cpython_suite
//! make cpython-test
//! ```
//!
//! Run a single test:
//!
//! ```
//! cargo test -p ferrython-cli --test cpython_suite test_bool
//! ```
//!
//! Tests marked `#[ignore]` are known to not yet pass; run them with
//! `cargo test ... -- --ignored` to see their current failure output.

use std::path::PathBuf;
use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ferrython_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ferrython"))
}

fn runner_script() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent() // crates/
        .unwrap()
        .parent() // workspace root
        .unwrap()
        .join("tools")
        .join("run_cpython_tests.py")
}

/// Invoke `ferrython tools/run_cpython_tests.py <test_name>` and return
/// `Ok(())` on success or `Err(output)` on failure.
fn run_cpython_test(test_name: &str) -> Result<(), String> {
    let bin = ferrython_bin();
    let runner = runner_script();

    let output = Command::new(&bin)
        .arg(&runner)
        .arg(test_name)
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", bin.display()));

    if output.status.success() {
        Ok(())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "{test_name} failed (exit {:?})\n\
             --- stdout ---\n{stdout}\
             --- stderr ---\n{stderr}",
            output.status.code()
        ))
    }
}

// ---------------------------------------------------------------------------
// Macro — generates one #[test] per CPython test file
// ---------------------------------------------------------------------------

macro_rules! cpython_test {
    ($name:ident) => {
        #[test]
        fn $name() {
            if let Err(msg) = run_cpython_test(stringify!($name)) {
                panic!("{msg}");
            }
        }
    };
    (#[ignore = $reason:expr] $name:ident) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            if let Err(msg) = run_cpython_test(stringify!($name)) {
                panic!("{msg}");
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Tests — sorted alphabetically by test name
//
// Remove `#[ignore = ...]` once Ferrython passes a given test file.
// Run ignored tests with:
//   cargo test -p ferrython-cli --test cpython_suite -- --ignored
// ---------------------------------------------------------------------------

cpython_test!(test_bool);
cpython_test!(test_complex);
cpython_test!(test_dict);
cpython_test!(test_enumerate);
cpython_test!(test_exception_hierarchy);
cpython_test!(test_float);
cpython_test!(test_fstring);
cpython_test!(test_functools);
cpython_test!(test_generators);
cpython_test!(test_int);
cpython_test!(test_isinstance);
cpython_test!(test_iter);
cpython_test!(test_operator);
cpython_test!(test_set);
cpython_test!(test_string);
