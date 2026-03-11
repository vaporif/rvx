//! Integration tests that hit the real crates.io API.
//! Run with: cargo test --test registry_test -- --ignored

#[test]
#[ignore] // requires network
fn test_fetch_known_crate() {
    let spec = rvx::cli::CrateSpec {
        name: "ripgrep".to_string(),
        version: None,
    };
    let info = rvx::registry::fetch(&spec).unwrap();
    assert_eq!(info.name, "ripgrep");
    assert!(!info.version.is_empty());
    assert!(info.repository.is_some());
}

#[test]
#[ignore]
fn test_fetch_nonexistent_crate() {
    let spec = rvx::cli::CrateSpec {
        name: "this-crate-definitely-does-not-exist-rvx-test".to_string(),
        version: None,
    };
    let result = rvx::registry::fetch(&spec);
    assert!(result.is_err());
}

#[test]
#[ignore]
fn test_fetch_with_binstall_metadata() {
    let spec = rvx::cli::CrateSpec {
        name: "cargo-binstall".to_string(),
        version: None,
    };
    let info = rvx::registry::fetch(&spec).unwrap();
    assert!(
        info.binstall.is_some(),
        "cargo-binstall should have binstall metadata"
    );
    let binstall = info.binstall.unwrap();
    assert!(!binstall.pkg_url.is_empty());
}
