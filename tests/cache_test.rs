use std::fs;
use tempfile::TempDir;

#[test]
fn test_cache_store_and_find() {
    let tmp = TempDir::new().unwrap();

    let bin_dir = tmp.path().join("bin");

    // Initially empty
    assert!(!bin_dir.exists());

    // Create a fake binary
    let fake_binary = tmp.path().join("fake-binary");
    fs::write(&fake_binary, b"#!/bin/sh\necho hello").unwrap();

    // Store it
    let meta = rvx::cache::CacheMeta {
        crate_name: "test-crate".to_string(),
        version: "1.0.0".to_string(),
        source_url: "https://example.com/test.tar.gz".to_string(),
        checksum: Some("abc123".to_string()),
        cached_at: "2026-01-01T00:00:00Z".to_string(),
    };

    let stored = rvx::cache::store_to(tmp.path(), &fake_binary, &meta).unwrap();
    assert!(stored.exists());
    assert_eq!(stored.file_name().unwrap(), "test-crate@1.0.0");

    // Find it
    let found = rvx::cache::find_in(tmp.path(), "test-crate", Some("1.0.0"));
    assert!(found.is_some());
    assert_eq!(found.unwrap(), stored);

    // Find without version returns the cached one
    let found = rvx::cache::find_in(tmp.path(), "test-crate", None);
    assert!(found.is_some());

    // Not found
    let found = rvx::cache::find_in(tmp.path(), "other-crate", None);
    assert!(found.is_none());

    // Verify no name collision: "test" should NOT match "test-crate"
    let found = rvx::cache::find_in(tmp.path(), "test", None);
    assert!(
        found.is_none(),
        "should not match 'test-crate' when searching for 'test'"
    );
}

#[test]
fn test_cache_list() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    fs::write(bin_dir.join("tool-a@1.0.0"), b"binary").unwrap();
    fs::write(bin_dir.join("tool-b@2.3.1"), b"binary").unwrap();

    let entries = rvx::cache::list_in(tmp.path()).unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_cache_clean() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    let meta_dir = tmp.path().join("meta");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&meta_dir).unwrap();

    fs::write(bin_dir.join("tool@1.0.0"), b"binary").unwrap();
    fs::write(meta_dir.join("tool@1.0.0.json"), b"{}").unwrap();

    rvx::cache::clean_in(tmp.path()).unwrap();

    assert!(fs::read_dir(&bin_dir).unwrap().next().is_none());
    assert!(fs::read_dir(&meta_dir).unwrap().next().is_none());
}
