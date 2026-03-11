//! End-to-end tests. Requires network.
//! Run with: cargo test --test e2e_test -- --ignored

use std::process::Command;

#[test]
#[ignore]
fn test_rvx_list_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rvx"))
        .env("RVX_HOME", tmp.path())
        .args(["--list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("No cached binaries"));
}

#[test]
#[ignore]
fn test_rvx_clean_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rvx"))
        .env("RVX_HOME", tmp.path())
        .args(["--clean"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
#[ignore]
fn test_rvx_nonexistent_crate() {
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rvx"))
        .env("RVX_HOME", tmp.path())
        .args(["this-crate-definitely-does-not-exist-rvx-test"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("not found"));
}
