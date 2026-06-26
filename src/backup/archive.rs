#![allow(dead_code)]
use anyhow::{Context, Result};
use glob::Pattern;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::{write::SimpleFileOptions, ZipWriter};

pub struct ArchiveConfig {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

pub fn create_zip_archive(
    server_dir: &Path,
    output_path: &Path,
    config: &ArchiveConfig,
) -> Result<u64> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(output_path)
        .with_context(|| format!("Failed to create archive: {}", output_path.display()))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));

    let exclude_patterns: Vec<Pattern> = config
        .exclude
        .iter()
        .filter_map(|p| Pattern::new(p).ok())
        .collect();

    let files = collect_files(server_dir, config, &exclude_patterns)?;
    let mut total_bytes = 0u64;

    for file_path in files {
        let relative = file_path
            .strip_prefix(server_dir)
            .with_context(|| format!("Failed to strip prefix: {}", file_path.display()))?;
        let name = relative.to_string_lossy();

        if file_path.is_file() {
            zip.start_file(name.as_ref(), options)
                .with_context(|| format!("Failed to start zip entry: {}", name))?;
            let mut f = File::open(&file_path)
                .with_context(|| format!("Failed to open: {}", file_path.display()))?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            total_bytes += buf.len() as u64;
            zip.write_all(&buf)?;
        } else if file_path.is_dir() {
            let dir_name = format!("{}/", name);
            zip.add_directory(dir_name, options)?;
        }
    }

    zip.finish().context("Failed to finalize zip archive")?;
    Ok(total_bytes)
}

fn collect_files(
    server_dir: &Path,
    config: &ArchiveConfig,
    exclude_patterns: &[Pattern],
) -> Result<Vec<PathBuf>> {
    let mut all_files = Vec::new();

    if config.include.is_empty() {
        walk_dir(server_dir, server_dir, exclude_patterns, &mut all_files)?;
    } else {
        for include_path in &config.include {
            let full = server_dir.join(include_path.trim_end_matches('/'));
            if full.is_dir() {
                walk_dir(&full, server_dir, exclude_patterns, &mut all_files)?;
            } else if full.is_file() {
                let relative = full.strip_prefix(server_dir).unwrap_or(&full);
                if !is_excluded(relative, exclude_patterns) {
                    all_files.push(full);
                }
            }
        }
    }

    all_files.sort();
    Ok(all_files)
}

fn walk_dir(
    dir: &Path,
    root: &Path,
    exclude_patterns: &[Pattern],
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("Cannot read directory: {}", dir.display()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path);

        if is_excluded(relative, exclude_patterns) {
            continue;
        }

        if path.is_dir() {
            files.push(path.clone());
            walk_dir(&path, root, exclude_patterns, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn is_excluded(relative: &Path, patterns: &[Pattern]) -> bool {
    let name = relative.to_string_lossy();
    for pattern in patterns {
        if pattern.matches(&name) {
            return true;
        }
        if let Some(file_name) = relative.file_name() {
            if pattern.matches(&file_name.to_string_lossy()) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_tree(root: &Path) {
        std::fs::create_dir_all(root.join("world")).unwrap();
        std::fs::write(root.join("world/level.dat"), b"data").unwrap();
        std::fs::write(root.join("server.log"), b"log").unwrap();
        std::fs::write(root.join("server.properties"), b"props").unwrap();
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::write(root.join("logs/latest.log"), b"log").unwrap();
    }

    #[test]
    fn test_include_only() {
        let dir = tempdir().unwrap();
        make_tree(dir.path());
        let out = dir.path().join("out.zip");
        let config = ArchiveConfig {
            include: vec!["world/".to_string(), "server.properties".to_string()],
            exclude: vec![],
        };
        create_zip_archive(dir.path(), &out, &config).unwrap();
        assert!(out.exists());
    }

    #[test]
    fn test_exclude_logs() {
        let dir = tempdir().unwrap();
        make_tree(dir.path());
        let out = dir.path().join("out2.zip");
        let config = ArchiveConfig {
            include: vec![],
            exclude: vec!["*.log".to_string(), "logs/".to_string()],
        };
        create_zip_archive(dir.path(), &out, &config).unwrap();
        assert!(out.exists());
    }

    #[test]
    fn test_empty_include_archives_all() {
        let dir = tempdir().unwrap();
        make_tree(dir.path());
        let out = dir.path().join("out3.zip");
        let config = ArchiveConfig {
            include: vec![],
            exclude: vec![],
        };
        create_zip_archive(dir.path(), &out, &config).unwrap();
        assert!(out.exists());
    }
}
