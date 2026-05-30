use std::collections::HashMap;
use std::fs;
use std::io::Write;

use camino::Utf8PathBuf;
use polars::prelude::*;
use tracing::debug;

use crate::core::error::{StageError, StageResult};

use super::merger::{EpisodeMapping, MergeConfig, SourceDataset, TaskRemapping};

/// Process all parquet files: read from sources, remap columns, and write to output.
pub fn process_parquet_files(
    chunks_size: usize,
    config: &MergeConfig,
    sources: &[SourceDataset],
    mappings: &[EpisodeMapping],
    task_remapping: &TaskRemapping,
) -> StageResult<()> {
    for mapping in mappings {
        let source = &sources[mapping.source_index];
        let old_chunk = mapping.old_episode_index / source.info.chunks_size;

        let src_path = source.path.join(format!(
            "data/chunk-{:03}/episode_{:06}.parquet",
            old_chunk, mapping.old_episode_index
        ));

        if !src_path.exists() {
            return Err(StageError::missing(format!(
                "parquet file not found: {}",
                src_path
            )));
        }

        debug!(
            source = %src_path,
            old_episode = mapping.old_episode_index,
            new_episode = mapping.new_episode_index,
            "processing parquet file"
        );

        let mut df = LazyFrame::scan_parquet(PlPath::new(src_path.as_str()), Default::default())?
            .collect()?;

        let task_map = &task_remapping.per_source_map[mapping.source_index];
        df = remap_parquet_columns(df, mapping, task_map)?;

        // Write to output
        let new_chunk = mapping.new_episode_index / chunks_size;
        let dst_path = config.output.join(format!(
            "data/chunk-{:03}/episode_{:06}.parquet",
            new_chunk, mapping.new_episode_index
        ));

        write_parquet(&dst_path, &mut df)?;
    }

    Ok(())
}

/// Remap the index columns in a parquet DataFrame.
///
/// All operations are eager (no lazy/eager switching) for simplicity.
fn remap_parquet_columns(
    mut df: DataFrame,
    mapping: &EpisodeMapping,
    task_map: &HashMap<i64, i64>,
) -> StageResult<DataFrame> {
    let height = df.height();

    // Replace episode_index with new value (constant column)
    let episode_values: Int64Chunked = (0..height)
        .map(|_| Some(mapping.new_episode_index as i64))
        .collect_ca("episode_index".into());
    df.replace("episode_index", episode_values.into_series())?;

    // Remap index = global_frame_offset + frame_index
    let frame_index = df.column("frame_index")?.i64()?;
    let offset = mapping.global_frame_offset as i64;
    let new_index: Int64Chunked = frame_index
        .into_iter()
        .map(|opt| opt.map(|v| v + offset))
        .collect_ca("index".into());
    df.replace("index", new_index.into_series())?;

    // Remap task index columns
    let task_columns = [
        "task_index",
        "primitive_action_index",
        "short_horizon_task_index",
    ];
    for col_name in &task_columns {
        if df.column(col_name).is_ok() {
            df = remap_task_column(df, col_name, task_map)?;
        }
    }

    Ok(df)
}

/// Remap a single task index column using the provided mapping.
fn remap_task_column(
    mut df: DataFrame,
    col_name: &str,
    task_map: &HashMap<i64, i64>,
) -> StageResult<DataFrame> {
    let column = df.column(col_name)?;
    let chunked = column.i64()?;

    let remapped: Int64Chunked = chunked
        .into_iter()
        .map(|opt_val| {
            opt_val.map(|old_idx| {
                // -1 indicates "no task" and should be preserved as-is
                if old_idx < 0 {
                    old_idx
                } else {
                    task_map.get(&old_idx).copied().unwrap_or(old_idx)
                }
            })
        })
        .collect_ca(col_name.into());

    df.replace(col_name, remapped.into_series())?;
    Ok(df)
}

/// Write a DataFrame to a parquet file, creating parent directories as needed.
fn write_parquet(path: &Utf8PathBuf, df: &mut DataFrame) -> StageResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent.as_std_path())?;
    }

    let mut file = fs::File::create(path.as_std_path())?;
    ParquetWriter::new(&mut file).finish(df)?;
    file.flush()?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::super::merger::EpisodeMapping;
    use super::*;

    fn make_test_df() -> DataFrame {
        DataFrame::new(vec![
            Column::new("frame_index".into(), &[0i64, 1, 2, 3, 4]),
            Column::new("episode_index".into(), &[0i64, 0, 0, 0, 0]),
            Column::new("index".into(), &[0i64, 1, 2, 3, 4]),
            Column::new("task_index".into(), &[0i64, 0, 1, 1, 0]),
            Column::new("primitive_action_index".into(), &[0i64, 0, 1, 1, 0]),
            Column::new("short_horizon_task_index".into(), &[-1i64, -1, -1, -1, -1]),
            Column::new("observation.state".into(), &[1.0f64, 2.0, 3.0, 4.0, 5.0]),
        ])
        .unwrap()
    }

    #[test]
    fn test_remap_episode_index() {
        let df = make_test_df();
        let mapping = EpisodeMapping {
            source_index: 0,
            old_episode_index: 0,
            new_episode_index: 3,
            frame_count: 5,
            global_frame_offset: 100,
        };
        let task_map = HashMap::from([(0, 0), (1, 1)]);
        let result = remap_parquet_columns(df, &mapping, &task_map).unwrap();

        let ep_col = result.column("episode_index").unwrap().i64().unwrap();
        assert!(ep_col.into_no_null_iter().all(|v| v == 3));
    }

    #[test]
    fn test_remap_global_index() {
        let df = make_test_df();
        let mapping = EpisodeMapping {
            source_index: 0,
            old_episode_index: 0,
            new_episode_index: 3,
            frame_count: 5,
            global_frame_offset: 100,
        };
        let task_map = HashMap::from([(0, 0), (1, 1)]);
        let result = remap_parquet_columns(df, &mapping, &task_map).unwrap();

        let idx_col = result.column("index").unwrap().i64().unwrap();
        let values: Vec<i64> = idx_col.into_no_null_iter().collect();
        assert_eq!(values, vec![100, 101, 102, 103, 104]);
    }

    #[test]
    fn test_remap_task_indices() {
        let df = make_test_df();
        let mapping = EpisodeMapping {
            source_index: 0,
            old_episode_index: 0,
            new_episode_index: 0,
            frame_count: 5,
            global_frame_offset: 0,
        };
        // Remap: 0->5, 1->10
        let task_map = HashMap::from([(0, 5), (1, 10)]);
        let result = remap_parquet_columns(df, &mapping, &task_map).unwrap();

        let task_col = result.column("task_index").unwrap().i64().unwrap();
        let values: Vec<i64> = task_col.into_no_null_iter().collect();
        assert_eq!(values, vec![5, 5, 10, 10, 5]);

        let pa_col = result
            .column("primitive_action_index")
            .unwrap()
            .i64()
            .unwrap();
        let pa_values: Vec<i64> = pa_col.into_no_null_iter().collect();
        assert_eq!(pa_values, vec![5, 5, 10, 10, 5]);
    }

    #[test]
    fn test_remap_preserves_data() {
        let df = make_test_df();
        let mapping = EpisodeMapping {
            source_index: 0,
            old_episode_index: 0,
            new_episode_index: 1,
            frame_count: 5,
            global_frame_offset: 50,
        };
        let task_map = HashMap::from([(0, 0), (1, 1)]);
        let result = remap_parquet_columns(df, &mapping, &task_map).unwrap();

        let obs_col = result.column("observation.state").unwrap().f64().unwrap();
        let values: Vec<f64> = obs_col.into_no_null_iter().collect();
        assert_eq!(values, vec![1.0, 2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn test_remap_negative_task_index() {
        let df = make_test_df();
        let mapping = EpisodeMapping {
            source_index: 0,
            old_episode_index: 0,
            new_episode_index: 0,
            frame_count: 5,
            global_frame_offset: 0,
        };
        let task_map = HashMap::from([(0, 5), (1, 10)]);
        let result = remap_parquet_columns(df, &mapping, &task_map).unwrap();

        // short_horizon_task_index should remain -1
        let sht_col = result
            .column("short_horizon_task_index")
            .unwrap()
            .i64()
            .unwrap();
        assert!(sht_col.into_no_null_iter().all(|v| v == -1));
    }
}
