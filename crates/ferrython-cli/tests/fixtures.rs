//! Integration tests: run each Python fixture file through the full pipeline
//! (parse → compile → execute) and assert it exits without error.
//!
//! Run with: `cargo test -p ferrython-cli --test fixtures`

use std::fs;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    // Walk up from crate dir to workspace root, then into tests/fixtures
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent() // crates/
        .unwrap()
        .parent() // workspace root
        .unwrap()
        .join("tests")
        .join("fixtures")
}

fn run_fixture(path: &std::path::Path) -> Result<(), String> {
    let source = fs::read_to_string(path)
        .map_err(|e| format!("read error: {e}"))?;
    let filename = path.file_name().unwrap().to_str().unwrap();

    let module = ferrython_parser::parse(&source, filename)
        .map_err(|e| format!("parse error: {e}"))?;
    let code = ferrython_compiler::compile(&module, filename)
        .map_err(|e| format!("compile error: {e}"))?;

    let mut vm = ferrython_vm::VirtualMachine::new();
    vm.execute(code)
        .map_err(|e| format!("runtime error: {e}"))?;

    Ok(())
}

// Macro to generate one #[test] per fixture file.
macro_rules! fixture_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let path = fixture_dir().join($file);
            if let Err(msg) = run_fixture(&path) {
                panic!("{}: {msg}", $file);
            }
        }
    };
    (#[ignore = $reason:expr] $name:ident, $file:expr) => {
        #[test]
        #[ignore = $reason]
        fn $name() {
            let path = fixture_dir().join($file);
            if let Err(msg) = run_fixture(&path) {
                panic!("{}: {msg}", $file);
            }
        }
    };
}

// Core test suites
fixture_test!(test_basics, "test_basics.py");
fixture_test!(test_advanced, "test_advanced.py");

// Phase tests
fixture_test!(test_phase2,  "test_phase2.py");
fixture_test!(test_phase3,  "test_phase3.py");
fixture_test!(test_phase4,  "test_phase4.py");
fixture_test!(test_phase5,  "test_phase5.py");
fixture_test!(test_phase6,  "test_phase6.py");
fixture_test!(test_phase7,  "test_phase7.py");
fixture_test!(test_phase8,  "test_phase8.py");
fixture_test!(test_phase9,  "test_phase9.py");
fixture_test!(test_phase10, "test_phase10.py");
fixture_test!(test_phase11, "test_phase11.py");
fixture_test!(test_phase12, "test_phase12.py");
fixture_test!(test_phase13, "test_phase13.py");
fixture_test!(test_phase14, "test_phase14.py");
fixture_test!(test_phase15, "test_phase15.py");
fixture_test!(test_phase16, "test_phase16.py");
fixture_test!(test_phase17, "test_phase17.py");
fixture_test!(test_phase18, "test_phase18.py");
fixture_test!(test_phase19, "test_phase19.py");
fixture_test!(test_phase20, "test_phase20.py");
fixture_test!(test_phase21, "test_phase21.py");
fixture_test!(test_phase22, "test_phase22.py");
fixture_test!(test_phase23, "test_phase23.py");
fixture_test!(test_phase24, "test_phase24.py");
fixture_test!(test_phase25, "test_phase25.py");
fixture_test!(test_phase26, "test_phase26.py");
fixture_test!(test_phase27, "test_phase27.py");
fixture_test!(test_phase28, "test_phase28.py");
fixture_test!(test_phase29, "test_phase29.py");
fixture_test!(test_phase30, "test_phase30.py");
fixture_test!(test_phase31, "test_phase31.py");
fixture_test!(test_phase32, "test_phase32.py");
fixture_test!(test_phase33, "test_phase33.py");
fixture_test!(test_phase34, "test_phase34.py");
fixture_test!(test_phase35, "test_phase35.py");
fixture_test!(test_phase36, "test_phase36.py");
fixture_test!(test_phase37, "test_phase37.py");
fixture_test!(test_phase38, "test_phase38.py");
fixture_test!(test_phase39, "test_phase39.py");
fixture_test!(test_phase40, "test_phase40.py");
fixture_test!(test_phase41, "test_phase41.py");
fixture_test!(test_phase42, "test_phase42.py");
fixture_test!(test_phase43, "test_phase43.py");
fixture_test!(test_phase44, "test_phase44.py");
fixture_test!(test_phase45, "test_phase45.py");
fixture_test!(test_phase46, "test_phase46.py");
fixture_test!(test_phase47, "test_phase47.py");
fixture_test!(test_phase48, "test_phase48.py");
fixture_test!(test_phase49, "test_phase49.py");
fixture_test!(test_phase50, "test_phase50.py");
fixture_test!(test_phase51, "test_phase51.py");
fixture_test!(test_phase52, "test_phase52.py");
fixture_test!(test_phase53, "test_phase53.py");
fixture_test!(test_phase54, "test_phase54.py");
fixture_test!(test_phase55, "test_phase55.py");
fixture_test!(test_phase56, "test_phase56.py");
fixture_test!(test_phase57, "test_phase57.py");
fixture_test!(test_phase58, "test_phase58.py");
fixture_test!(test_phase59, "test_phase59.py");
fixture_test!(test_phase60, "test_phase60.py");
fixture_test!(test_phase61, "test_phase61.py");
fixture_test!(test_phase62, "test_phase62.py");
fixture_test!(test_phase63, "test_phase63.py");
fixture_test!(test_phase64, "test_phase64.py");

// Expand tests
fixture_test!(test_expand17, "test_expand17.py");
fixture_test!(test_expand24, "test_expand24.py");
fixture_test!(test_expand26, "test_expand26.py");
fixture_test!(test_expand28, "test_expand28.py");
fixture_test!(test_expand29, "test_expand29.py");
fixture_test!(test_expand30, "test_expand30.py");

// CPython compatibility tests
fixture_test!(test_cpython_compat82, "test_cpython_compat82.py");
fixture_test!(test_cpython_compat83, "test_cpython_compat83.py");
fixture_test!(test_cpython_compat84, "test_cpython_compat84.py");
fixture_test!(test_cpython_compat85, "test_cpython_compat85.py");
fixture_test!(test_cpython_compat86, "test_cpython_compat86.py");
fixture_test!(test_cpython_compat87, "test_cpython_compat87.py");
fixture_test!(test_cpython_compat88, "test_cpython_compat88.py");
fixture_test!(test_cpython_compat89, "test_cpython_compat89.py");
fixture_test!(test_cpython_compat90, "test_cpython_compat90.py");
fixture_test!(test_cpython_compat91, "test_cpython_compat91.py");
fixture_test!(test_cpython_compat92, "test_cpython_compat92.py");
fixture_test!(test_cpython_compat93, "test_cpython_compat93.py");
fixture_test!(test_cpython_compat94, "test_cpython_compat94.py");
fixture_test!(test_cpython_compat95, "test_cpython_compat95.py");
fixture_test!(test_cpython_compat96, "test_cpython_compat96.py");
fixture_test!(test_cpython_compat97, "test_cpython_compat97.py");
fixture_test!(test_cpython_compat98, "test_cpython_compat98.py");
fixture_test!(test_cpython_compat99, "test_cpython_compat99.py");
fixture_test!(test_cpython_compat100, "test_cpython_compat100.py");
fixture_test!(test_cpython_compat101, "test_cpython_compat101.py");
