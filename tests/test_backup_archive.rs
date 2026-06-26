use anvil::backup::archive::{create_zip_archive, ArchiveConfig};
use std::fs::File;
use tempfile::tempdir;
use zip::ZipArchive;

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

fn zip_names(path: &std::path::Path) -> Vec<String> {
    let file = File::open(path).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut names = Vec::new();
    for i in 0..archive.len() {
        names.push(archive.by_index(i).unwrap().name().to_string());
    }
    names
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
    let names = zip_names(&out);
    assert!(names.iter().any(|name| name == "world/level.dat"));
    assert!(names.iter().any(|name| name == "server.properties"));
    assert!(!names.iter().any(|name| name == "server.log"));
}

#[test]
fn test_archive_exclude_logs() {
    let dir = tempdir().unwrap();
    setup_tree(dir.path());
    let out = dir.path().join("backup2.zip");
    let config = ArchiveConfig {
        include: vec![],
        exclude: vec![
            "*.log".to_string(),
            "logs/".to_string(),
            "cache/".to_string(),
        ],
    };
    create_zip_archive(dir.path(), &out, &config).unwrap();
    assert!(out.exists());
    let names = zip_names(&out);
    assert!(!names.iter().any(|name| name == "logs/latest.log"));
    assert!(!names.iter().any(|name| name == "cache/data.bin"));
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
    let names = zip_names(&out);
    assert!(!names.iter().any(|name| name == "backup3.zip"));
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
    let names = zip_names(&out);
    assert!(names.iter().any(|name| name == "world/level.dat"));
    assert!(!names.iter().any(|name| name == "world/region.mca"));
}

#[test]
fn test_archive_directory_exclude_without_file_glob() {
    let dir = tempdir().unwrap();
    setup_tree(dir.path());
    let out = dir.path().join("backup5.zip");
    let config = ArchiveConfig {
        include: vec![],
        exclude: vec!["cache/".to_string()],
    };
    create_zip_archive(dir.path(), &out, &config).unwrap();
    let names = zip_names(&out);
    assert!(!names.iter().any(|name| name == "cache/data.bin"));
}
