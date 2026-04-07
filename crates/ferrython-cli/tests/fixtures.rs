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
fixture_test!(test_phase65, "test_phase65.py");
fixture_test!(test_phase66, "test_phase66.py");
fixture_test!(test_phase67, "test_phase67.py");
fixture_test!(test_phase68, "test_phase68.py");
fixture_test!(test_phase69, "test_phase69.py");
fixture_test!(test_phase70, "test_phase70.py");
fixture_test!(test_phase71, "test_phase71.py");
fixture_test!(test_phase72, "test_phase72.py");

// Expand tests
fixture_test!(test_expand17, "test_expand17.py");
fixture_test!(test_expand24, "test_expand24.py");
fixture_test!(test_expand26, "test_expand26.py");
fixture_test!(test_expand28, "test_expand28.py");
fixture_test!(test_expand29, "test_expand29.py");
fixture_test!(test_expand30, "test_expand30.py");
fixture_test!(test_expand31, "test_expand31.py");

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

// Phase 73: __contains__, __missing__, __len__/__bool__ protocols
fixture_test!(test_phase73, "test_phase73.py");

// Phase 74: Pure Python stdlib (reprlib) + Rust stdlib verification
fixture_test!(test_phase74, "test_phase74.py");

// Phase 75: Pure Python stdlib modules (colorsys, gettext, keyword) + Rust decimal/datetime improvements
fixture_test!(test_phase75, "test_phase75.py");

// Phase 76: More stdlib modules — unittest improvements + json enhancements
fixture_test!(test_phase76, "test_phase76.py");
fixture_test!(test_phase77, "test_phase77.py");
fixture_test!(test_phase78, "test_phase78.py");
fixture_test!(test_phase79, "test_phase79.py");
fixture_test!(test_phase80, "test_phase80.py");
fixture_test!(test_phase81, "test_phase81.py");
fixture_test!(test_phase82, "test_phase82.py");
fixture_test!(test_phase83, "test_phase83.py");
fixture_test!(test_phase84, "test_phase84.py");
fixture_test!(test_phase85, "test_phase85.py");
fixture_test!(test_phase86, "test_phase86.py");
fixture_test!(test_phase87, "test_phase87.py");
fixture_test!(test_phase88, "test_phase88.py");
fixture_test!(test_phase89, "test_phase89.py");
fixture_test!(test_phase90, "test_phase90.py");
fixture_test!(test_phase91, "test_phase91.py");
fixture_test!(test_phase92, "test_phase92.py");
fixture_test!(test_phase93, "test_phase93.py");
fixture_test!(test_phase94, "test_phase94.py");
fixture_test!(test_phase95, "test_phase95.py");
fixture_test!(test_phase96, "test_phase96.py");
fixture_test!(test_phase97, "test_phase97.py");
fixture_test!(test_phase98, "test_phase98.py");
fixture_test!(test_phase99, "test_phase99.py");
fixture_test!(test_phase100, "test_phase100.py");
fixture_test!(test_phase101, "test_phase101.py");
fixture_test!(test_phase102, "test_phase102.py");
fixture_test!(test_phase103, "test_phase103.py");
fixture_test!(test_phase104, "test_phase104.py");
fixture_test!(test_phase105, "test_phase105.py");
fixture_test!(test_phase106, "test_phase106.py");

fixture_test!(test_phase107, "test_phase107.py");
fixture_test!(test_phase108, "test_phase108.py");
fixture_test!(test_phase109, "test_phase109.py");
fixture_test!(test_phase110, "test_phase110.py");
fixture_test!(test_phase111, "test_phase111.py");
fixture_test!(test_phase112, "test_phase112.py");
fixture_test!(test_phase113, "test_phase113.py");
fixture_test!(test_phase114, "test_phase114.py");
fixture_test!(test_phase115, "test_phase115.py");
fixture_test!(test_phase116, "test_phase116.py");
fixture_test!(test_phase117, "test_phase117.py");
fixture_test!(test_phase118, "test_phase118.py");
fixture_test!(test_phase119, "test_phase119.py");
fixture_test!(test_phase120, "test_phase120.py");
fixture_test!(test_phase121, "test_phase121.py");
fixture_test!(test_phase122, "test_phase122.py");
fixture_test!(test_phase123, "test_phase123.py");
fixture_test!(test_phase124, "test_phase124.py");
fixture_test!(test_phase125, "test_phase125.py");
fixture_test!(test_phase126, "test_phase126.py");
fixture_test!(test_phase127, "test_phase127.py");
fixture_test!(test_phase128, "test_phase128.py");
fixture_test!(test_phase129, "test_phase129.py");
fixture_test!(test_phase130, "test_phase130.py");
fixture_test!(test_phase131, "test_phase131.py");
fixture_test!(test_phase132, "test_phase132.py");
fixture_test!(test_phase133, "test_phase133.py");
fixture_test!(test_phase134, "test_phase134.py");
fixture_test!(test_phase135, "test_phase135.py");
fixture_test!(test_phase136, "test_phase136.py");
fixture_test!(test_phase137, "test_phase137.py");
fixture_test!(test_phase138, "test_phase138.py");
fixture_test!(test_phase139, "test_phase139.py");
fixture_test!(test_phase140, "test_phase140.py");
fixture_test!(test_phase141, "test_phase141.py");
fixture_test!(test_phase142, "test_phase142.py");
fixture_test!(test_phase143, "test_phase143.py");
fixture_test!(test_phase144, "test_phase144.py");
fixture_test!(test_phase145, "test_phase145.py");
fixture_test!(test_phase146, "test_phase146.py");
fixture_test!(test_phase147, "test_phase147.py");
fixture_test!(test_phase148, "test_phase148.py");
fixture_test!(test_phase149, "test_phase149.py");
