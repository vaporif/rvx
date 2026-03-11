//! End-to-end tests. Requires network + built binary.
//! Run with: cargo test --test e2e_test -- --ignored

use std::process::Command;

fn rvx() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rvx"))
}

fn rvx_with_home(home: &std::path::Path) -> Command {
    let mut cmd = rvx();
    cmd.env("RVX_HOME", home);
    cmd
}

// ---------------------------------------------------------------------------
// Cache management (offline)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_list_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let out = rvx_with_home(tmp.path()).args(["--list"]).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("No cached binaries"));
}

#[test]
#[ignore]
fn test_clean_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let out = rvx_with_home(tmp.path())
        .args(["--clean"])
        .output()
        .unwrap();
    assert!(out.status.success());
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_nonexistent_crate() {
    let tmp = tempfile::tempdir().unwrap();
    let out = rvx_with_home(tmp.path())
        .args(["this-crate-definitely-does-not-exist-rvx-test"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found"), "stderr: {stderr}");
}

#[test]
#[ignore]
fn test_nonexistent_version() {
    let tmp = tempfile::tempdir().unwrap();
    let out = rvx_with_home(tmp.path())
        .args(["bat@99999.0.0"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not found"),
        "expected version not found error, got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Download + run (network required)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_download_and_run() {
    let tmp = tempfile::tempdir().unwrap();
    let out = rvx_with_home(tmp.path())
        .args(["bat", "--", "--version"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("bat"), "stdout: {stdout}");
}

#[test]
#[ignore]
fn test_download_specific_version() {
    let tmp = tempfile::tempdir().unwrap();
    let out = rvx_with_home(tmp.path())
        .args(["bat@0.25.0", "--", "--version"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("0.25.0"),
        "expected version 0.25.0, got: {stdout}"
    );
}

#[test]
#[ignore]
fn test_download_with_bin_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let out = rvx_with_home(tmp.path())
        .args(["ripgrep", "--bin", "rg", "--", "--version"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("ripgrep"), "stdout: {stdout}");
}

// ---------------------------------------------------------------------------
// Cache behavior (network required for first run)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_cache_hit() {
    let tmp = tempfile::tempdir().unwrap();

    // First run: downloads
    let out1 = rvx_with_home(tmp.path())
        .args(["bat", "--", "--version"])
        .output()
        .unwrap();
    assert!(
        out1.status.success(),
        "first run failed: {}",
        String::from_utf8_lossy(&out1.stderr)
    );
    let stderr1 = String::from_utf8_lossy(&out1.stderr);
    assert!(
        stderr1.contains("Downloading"),
        "first run should download: {stderr1}"
    );

    // Second run: cached (no download output)
    let out2 = rvx_with_home(tmp.path())
        .args(["bat", "--", "--version"])
        .output()
        .unwrap();
    assert!(
        out2.status.success(),
        "second run failed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let stderr2 = String::from_utf8_lossy(&out2.stderr);
    assert!(
        !stderr2.contains("Downloading"),
        "second run should use cache: {stderr2}"
    );

    // Both should produce same output
    assert_eq!(out1.stdout, out2.stdout);
}

#[test]
#[ignore]
fn test_list_shows_installed() {
    let tmp = tempfile::tempdir().unwrap();

    // Install something
    let out = rvx_with_home(tmp.path())
        .args(["bat@0.25.0", "--", "--version"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // List should show it
    let list = rvx_with_home(tmp.path()).args(["--list"]).output().unwrap();
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("bat@0.25.0"), "list output: {stdout}");
}

#[test]
#[ignore]
fn test_clean_removes_cached() {
    let tmp = tempfile::tempdir().unwrap();

    // Install something
    let out = rvx_with_home(tmp.path())
        .args(["bat", "--", "--version"])
        .output()
        .unwrap();
    assert!(out.status.success());

    // Verify it's listed
    let list = rvx_with_home(tmp.path()).args(["--list"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("bat@"), "should be cached: {stdout}");

    // Clean
    let clean = rvx_with_home(tmp.path())
        .args(["--clean"])
        .output()
        .unwrap();
    assert!(clean.status.success());

    // List should be empty
    let list2 = rvx_with_home(tmp.path()).args(["--list"]).output().unwrap();
    let stdout2 = String::from_utf8_lossy(&list2.stdout);
    assert!(
        stdout2.contains("No cached binaries"),
        "should be empty after clean: {stdout2}"
    );
}

#[test]
#[ignore]
fn test_update_redownloads() {
    let tmp = tempfile::tempdir().unwrap();

    // First install
    let out1 = rvx_with_home(tmp.path())
        .args(["bat", "--", "--version"])
        .output()
        .unwrap();
    assert!(out1.status.success());

    // Update: should re-download even though cached
    let out2 = rvx_with_home(tmp.path())
        .args(["--update", "bat", "--", "--version"])
        .output()
        .unwrap();
    assert!(
        out2.status.success(),
        "update failed: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let stderr2 = String::from_utf8_lossy(&out2.stderr);
    assert!(
        stderr2.contains("Downloading"),
        "update should re-download: {stderr2}"
    );
}

#[test]
#[ignore]
fn test_quiet_suppresses_output() {
    let tmp = tempfile::tempdir().unwrap();
    let out = rvx_with_home(tmp.path())
        .args(["--quiet", "bat", "--", "--version"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("Downloading"),
        "quiet should suppress download output: {stderr}"
    );
    assert!(
        !stderr.contains("Found binary"),
        "quiet should suppress resolution output: {stderr}"
    );
}
