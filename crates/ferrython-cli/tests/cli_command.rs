//! CLI command-string behavior.

use std::process::Command;

fn ferrython_bin() -> &'static str {
    env!("CARGO_BIN_EXE_ferrython")
}

#[test]
fn command_accepts_backslash_n_between_statements() {
    let output = Command::new(ferrython_bin())
        .arg("-c")
        .arg("print(1)\\nprint(2)")
        .output()
        .expect("failed to run ferrython -c");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n2\n");
}

#[test]
fn command_does_not_decode_backslash_n_inside_strings() {
    let output = Command::new(ferrython_bin())
        .arg("-c")
        .arg("print('a\\nb')\\nprint('done')")
        .output()
        .expect("failed to run ferrython -c");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "a\nb\ndone\n");
}
