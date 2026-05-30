use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use walkdir::WalkDir;

const PARQUET_DIR_NAME: &str = "parquet";
const METADATA_FILE_NAME: &str = "_metadata.parquet";
const TOPIC_TYPE_MAP_FILE_NAME: &str = "_topic_type_map.parquet";

pub fn collect_bundle_roots_from_paths(paths: &[Utf8PathBuf]) -> Result<Vec<Utf8PathBuf>> {
    let mut bundle_roots = Vec::new();

    for path in paths {
        let metadata =
            std::fs::metadata(path).with_context(|| format!("failed to access {}", path))?;
        if metadata.is_file() {
            anyhow::bail!("bundle input must be a directory: {}", path);
        }
        if !metadata.is_dir() {
            anyhow::bail!("bundle path must be a file or directory: {}", path);
        }

        if is_bundle_root(path) {
            bundle_roots.push(path.clone());
            continue;
        }

        let discovered = discover_bundle_roots(path)?;
        if discovered.is_empty() {
            anyhow::bail!(
                "no rebake bundle directories found under: {} (expected directories containing {}/{})",
                path,
                PARQUET_DIR_NAME,
                TOPIC_TYPE_MAP_FILE_NAME
            );
        }
        bundle_roots.extend(discovered);
    }

    bundle_roots.sort();
    bundle_roots.dedup();

    if bundle_roots.is_empty() {
        anyhow::bail!("no bundle directories provided");
    }

    Ok(bundle_roots)
}

fn discover_bundle_roots(path: &Utf8Path) -> Result<Vec<Utf8PathBuf>> {
    let mut bundle_roots = Vec::new();

    for entry in WalkDir::new(path.as_std_path()) {
        let entry = entry?;
        if !entry.file_type().is_dir() {
            continue;
        }

        let candidate = Utf8PathBuf::from_path_buf(entry.into_path())
            .map_err(|_| anyhow::anyhow!("encountered non-UTF-8 path while collecting bundles"))?;

        if is_bundle_root(&candidate) {
            bundle_roots.push(candidate);
        }
    }

    Ok(bundle_roots)
}

fn is_bundle_root(path: &Utf8Path) -> bool {
    let parquet_dir = path.join(PARQUET_DIR_NAME);
    parquet_dir.join(TOPIC_TYPE_MAP_FILE_NAME).exists()
        && parquet_dir.join(METADATA_FILE_NAME).exists()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn collect_bundle_roots_accepts_directories() {
        let temp_dir = TempDir::new().unwrap();
        let bundle_a = Utf8PathBuf::from_path_buf(temp_dir.path().join("bundle_a")).unwrap();
        let bundle_b = Utf8PathBuf::from_path_buf(temp_dir.path().join("bundle_b")).unwrap();
        fs::create_dir_all(bundle_a.join(PARQUET_DIR_NAME).as_std_path()).unwrap();
        fs::create_dir_all(bundle_b.join(PARQUET_DIR_NAME).as_std_path()).unwrap();
        fs::write(
            bundle_a
                .join(PARQUET_DIR_NAME)
                .join(TOPIC_TYPE_MAP_FILE_NAME)
                .as_std_path(),
            b"map",
        )
        .unwrap();
        fs::write(
            bundle_a
                .join(PARQUET_DIR_NAME)
                .join(METADATA_FILE_NAME)
                .as_std_path(),
            b"meta",
        )
        .unwrap();
        fs::write(
            bundle_b
                .join(PARQUET_DIR_NAME)
                .join(TOPIC_TYPE_MAP_FILE_NAME)
                .as_std_path(),
            b"map",
        )
        .unwrap();
        fs::write(
            bundle_b
                .join(PARQUET_DIR_NAME)
                .join(METADATA_FILE_NAME)
                .as_std_path(),
            b"meta",
        )
        .unwrap();

        let roots = collect_bundle_roots_from_paths(&[
            bundle_b.clone(),
            bundle_a.clone(),
            bundle_a.clone(),
        ])
        .unwrap();

        assert_eq!(roots, vec![bundle_a, bundle_b]);
    }

    #[test]
    fn collect_bundle_roots_rejects_files() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = Utf8PathBuf::from_path_buf(temp_dir.path().join("not_a_dir")).unwrap();
        fs::write(file_path.as_std_path(), b"data").unwrap();

        let err = collect_bundle_roots_from_paths(&[file_path]).unwrap_err();
        assert!(err.to_string().contains("bundle input must be a directory"));
    }

    #[test]
    fn collect_bundle_roots_discovers_nested_bundles() {
        let temp_dir = TempDir::new().unwrap();
        let export_root = Utf8PathBuf::from_path_buf(temp_dir.path().join("exports")).unwrap();
        let bundle_a = export_root.join("uuid-a");
        let bundle_b = export_root.join("nested").join("uuid-b");
        fs::create_dir_all(bundle_a.join(PARQUET_DIR_NAME).as_std_path()).unwrap();
        fs::create_dir_all(bundle_b.join(PARQUET_DIR_NAME).as_std_path()).unwrap();

        for bundle in [&bundle_a, &bundle_b] {
            fs::write(
                bundle
                    .join(PARQUET_DIR_NAME)
                    .join(TOPIC_TYPE_MAP_FILE_NAME)
                    .as_std_path(),
                b"map",
            )
            .unwrap();
            fs::write(
                bundle
                    .join(PARQUET_DIR_NAME)
                    .join(METADATA_FILE_NAME)
                    .as_std_path(),
                b"meta",
            )
            .unwrap();
        }

        let roots = collect_bundle_roots_from_paths(&[export_root]).unwrap();
        assert_eq!(roots, vec![bundle_b, bundle_a]);
    }
}
