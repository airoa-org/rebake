use std::collections::HashMap;

use camino::{Utf8Path, Utf8PathBuf};
use polars::prelude::{LazyFrame, PlPath};

use crate::common::get_file_paths_from_dir;
use crate::core::stage::StageError;

/// Loads parquet frames from a directory.
///
/// Returns a HashMap where keys are relative paths from dir_path without extensions.
///
/// # Errors
///
/// Returns `StageError::External` if the parquet scan initialization fails
/// (e.g., invalid path format). Note that file content validation is lazy
/// (Polars `scan_parquet` behavior) and errors may occur when the returned
/// `LazyFrame` is collected. Use [`scan_parquet_frames`] for eager validation.
pub fn load_parquet_frames<P: AsRef<Utf8Path>>(
    dir_path: P,
) -> Result<HashMap<Utf8PathBuf, LazyFrame>, StageError> {
    let dir_path_ref = dir_path.as_ref();
    let mut result = HashMap::new();

    for relative_path_without_ext in get_file_paths_from_dir(dir_path_ref, "parquet") {
        let mut full_path = dir_path_ref.join(&relative_path_without_ext);
        full_path.set_extension("parquet");
        let lazy_frame =
            LazyFrame::scan_parquet(PlPath::new(full_path.as_str()), Default::default()).map_err(
                |e| StageError::external(format!("failed to read parquet file: {}", full_path), e),
            )?;
        result.insert(relative_path_without_ext, lazy_frame);
    }

    Ok(result)
}

/// Scans parquet frames from a directory, filtering out unreadable files.
///
/// This function is similar to `load_parquet_frames`, but filters out parquet files
/// that cannot be read (e.g., corrupted or incomplete files).
///
/// # Errors
///
/// Returns `StageError::External` if the initial parquet scan fails.
pub fn scan_parquet_frames<P: AsRef<Utf8Path>>(
    dir_path: P,
) -> Result<HashMap<Utf8PathBuf, LazyFrame>, StageError> {
    let frames = load_parquet_frames(dir_path)?;
    Ok(frames
        .into_iter()
        .filter(|(_, frame)| frame.clone().limit(1).collect().is_ok())
        .collect())
}
