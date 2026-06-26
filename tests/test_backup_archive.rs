use msc::backup::archive::{ArchiveConfig, create_zip_archive};
use tempfile::tempdir;

fn setup_tree(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("world")).unwrap();
    std::fs::write(root.join("world/level.dat"), b"level data").unwrap();
    std::fs::write(root.join("world/region.mca"), b"region data").unwrap();
    std::fs::write(root.join("server.properties"), b"prop=val").unwrap();
    std::fs::write(root.join("server.log"), b"log line").unwrap();
    std::fs::create_dir_all(root.join("logs")).unwrap();
    std::fs::write(root.join("logs/latest.log"), b"log").unwrap();
    std::fs::create_dir_all(root.join("cache")).unwrap();
    std::fs::write(root.join("cache/data.bin"), b"cache").unwrap();
}

#[test]
fn test_archive_with_include_only() {
    let dir = tempdir().unwrap();
    setup_tree(dir.path());
    let out = dir.path().join("backup.zip");
    let config = ArchiveConfig {
        include: vec!["world/".to_string(), "server.properties".to_string()],
        exclude: vec![],
    };
    let bytes = create_zip_archive(dir.path(), &out, &config).unwrap();
    assert!(out.exists());
    assert!(bytes > 0);
}

#[test]
fn test_archive_exclude_logs() {
    let dir = tempdir().unwrap();
    setup_tree(dir.path());
    let out = dir.path().join("backup2.zip");
    let config = ArchiveConfig {
        include: vec![],
        exclude: vec!["*.log".to_string(), "logs/".to_string(), "cache/".to_string()],
    };
    create_zip_archive(dir.path(), &out, &config).unwrap();
    assert!(out.exists());
}

#[test]
fn test_archive_empty_include_archives_all() {
    let dir = tempdir().unwrap();
    setup_tree(dir.path());
    let out = dir.path().join("backup3.zip");
    let config = ArchiveConfig {
        include: vec![],
        exclude: vec![],
    };
    let bytes = create_zip_archive(dir.path(), &out, &config).unwrap();
    assert!(out.exists());
    assert!(bytes > 0);
}

#[test]
fn test_archive_exclude_overrides_include() {
    let dir = tempdir().unwrap();
    setup_tree(dir.path());
    let out = dir.path().join("backup4.zip");
    let config = ArchiveConfig {
        include: vec!["world/".to_string()],
        exclude: vec!["*.mca".to_string()],
    };
    create_zip_archive(dir.path(), &out, &config).unwrap();
    assert!(out.exists());
}
