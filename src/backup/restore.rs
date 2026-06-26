#![allow(dead_code)]
use anyhow::{Context, Result};
use std::path::Path;
use zip::ZipArchive;

pub fn extract_zip(archive_path: &Path, target_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("Failed to open archive: {}", archive_path.display()))?;
    let mut archive = ZipArchive::new(file).context("Failed to read zip archive")?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let entry_name = entry.name().to_string();
        let Some(enclosed_name) = entry.enclosed_name() else {
            tracing::warn!(entry = %entry_name, "Skipping unsafe zip entry");
            continue;
        };
        let out_path = target_dir.join(enclosed_name);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)
                .with_context(|| format!("Failed to create {}", out_path.display()))?;
            std::io::copy(&mut entry, &mut out_file)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::{write::SimpleFileOptions, ZipWriter};

    #[test]
    fn test_extract_zip_skips_path_traversal_entries() {
        let dir = tempdir().unwrap();
        let archive_path = dir.path().join("restore.zip");
        let target_dir = dir.path().join("target");

        let file = std::fs::File::create(&archive_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("../escape.txt", options).unwrap();
        zip.write_all(b"escaped").unwrap();
        zip.start_file("world/level.dat", options).unwrap();
        zip.write_all(b"level").unwrap();
        zip.finish().unwrap();

        extract_zip(&archive_path, &target_dir).unwrap();

        assert!(!dir.path().join("escape.txt").exists());
        assert_eq!(
            std::fs::read(target_dir.join("world/level.dat")).unwrap(),
            b"level".to_vec()
        );
    }
}
