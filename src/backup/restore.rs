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
        let out_path = target_dir.join(entry.name());

        if entry.name().ends_with('/') {
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
