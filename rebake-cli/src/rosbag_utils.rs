//! Utilities for rosbag file collection and validation.
//!
//! This module provides functions for discovering and validating rosbag files
//! from file paths or directories.

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use walkdir::WalkDir;

/// Collect rosbag files from a path.
///
/// If the path is a file with .bag or .mcap extension, returns a vec with just that file.
/// If the path is a directory, recursively collects all .bag and .mcap files.
///
/// # Arguments
/// * `path` - A file or directory path
///
/// # Returns
/// A sorted list of rosbag file paths
///
/// # Errors
/// Returns an error if:
/// - The path doesn't exist or can't be accessed
/// - The path is a file without .bag or .mcap extension
/// - The directory contains no rosbag files
/// - A backup file (.orig.bag, .orig.mcap) is provided as a single file
pub fn collect_rosbags(path: &Utf8Path) -> Result<Vec<Utf8PathBuf>> {
    let metadata = std::fs::metadata(path).with_context(|| format!("failed to access {}", path))?;

    if metadata.is_file() {
        ensure_bag_extension(path)?;
        ensure_not_backup(path)?;
        return Ok(vec![path.to_owned()]);
    }

    if metadata.is_dir() {
        let mut bags = Vec::new();
        for entry in WalkDir::new(path.as_std_path()) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let entry_path = entry.path();
            let ext_str = entry_path.extension().and_then(|ext| ext.to_str());
            if ext_str != Some("bag") && ext_str != Some("mcap") {
                continue;
            }

            // Skip backup files
            if entry_path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".orig.bag") || name.ends_with(".orig.mcap"))
            {
                continue;
            }

            let utf8_path = Utf8PathBuf::from_path_buf(entry.into_path()).map_err(|_| {
                anyhow::anyhow!("encountered non-UTF-8 path while collecting rosbag files")
            })?;
            bags.push(utf8_path);
        }

        bags.sort();
        if bags.is_empty() {
            anyhow::bail!("no rosbag files found under directory: {}", path);
        }
        return Ok(bags);
    }

    anyhow::bail!("rosbag path must be a file or directory: {}", path);
}

/// Collect rosbags from multiple input paths.
///
/// Each input path can be either a file or directory. Directories are
/// recursively traversed to find all rosbag files.
///
/// # Returns
/// A deduplicated and sorted list of all rosbag file paths
pub fn collect_rosbags_from_paths(paths: &[Utf8PathBuf]) -> Result<Vec<Utf8PathBuf>> {
    let mut all_bags = Vec::new();

    for path in paths {
        let bags = collect_rosbags(path)?;
        all_bags.extend(bags);
    }

    // Deduplicate (in case of overlapping paths)
    all_bags.sort();
    all_bags.dedup();

    Ok(all_bags)
}

/// Validates that a path has a .bag or .mcap extension.
fn ensure_bag_extension(path: &Utf8Path) -> Result<()> {
    match path.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("bag") || ext.eq_ignore_ascii_case("mcap") => Ok(()),
        _ => anyhow::bail!("rosbag files must have .bag or .mcap extension: {}", path),
    }
}

/// Validates that a path is not a backup file (created by rosbag reindex).
fn ensure_not_backup(path: &Utf8Path) -> Result<()> {
    if path.as_str().ends_with(".orig.bag") || path.as_str().ends_with(".orig.mcap") {
        anyhow::bail!(
            "ignoring backup rosbag produced by rosbag reindex: {}",
            path
        );
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::TempDir;

    fn create_test_file(dir: &std::path::Path, name: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        File::create(path).unwrap();
    }

    #[test]
    fn test_collect_single_mcap_file() {
        let temp_dir = TempDir::new().unwrap();
        create_test_file(temp_dir.path(), "test.mcap");

        let path = Utf8PathBuf::from_path_buf(temp_dir.path().join("test.mcap")).unwrap();
        let result = collect_rosbags(&path).unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("test.mcap"));
    }

    #[test]
    fn test_collect_single_bag_file() {
        let temp_dir = TempDir::new().unwrap();
        create_test_file(temp_dir.path(), "test.bag");

        let path = Utf8PathBuf::from_path_buf(temp_dir.path().join("test.bag")).unwrap();
        let result = collect_rosbags(&path).unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("test.bag"));
    }

    #[test]
    fn test_collect_from_directory() {
        let temp_dir = TempDir::new().unwrap();
        create_test_file(temp_dir.path(), "a.mcap");
        create_test_file(temp_dir.path(), "b.bag");
        create_test_file(temp_dir.path(), "subdir/c.mcap");
        create_test_file(temp_dir.path(), "other.txt"); // Should be ignored

        let path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let result = collect_rosbags(&path).unwrap();

        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_skip_backup_files() {
        let temp_dir = TempDir::new().unwrap();
        create_test_file(temp_dir.path(), "test.mcap");
        create_test_file(temp_dir.path(), "test.orig.mcap"); // Should be skipped
        create_test_file(temp_dir.path(), "test.orig.bag"); // Should be skipped

        let path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let result = collect_rosbags(&path).unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("test.mcap"));
    }

    #[test]
    fn test_reject_wrong_extension() {
        let temp_dir = TempDir::new().unwrap();
        create_test_file(temp_dir.path(), "test.txt");

        let path = Utf8PathBuf::from_path_buf(temp_dir.path().join("test.txt")).unwrap();
        let result = collect_rosbags(&path);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("extension"));
    }

    #[test]
    fn test_empty_directory() {
        let temp_dir = TempDir::new().unwrap();

        let path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let result = collect_rosbags(&path);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no rosbag files"));
    }

    #[test]
    fn test_nonexistent_path() {
        let path = Utf8PathBuf::from("/nonexistent/path/to/file.mcap");
        let result = collect_rosbags(&path);

        assert!(result.is_err());
    }
}
