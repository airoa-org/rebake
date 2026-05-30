use std::collections::HashMap;

use polars::prelude::*;

use super::nearest_neighbor_time_synchronizer::NearestNeighborTimeSynchronizer;
use super::time_synchronizer::{TimeSynchronizer, TopicFrames};
use super::zero_order_hold_time_synchronizer::ZeroOrderHoldTimeSynchronizer;
use crate::core::error::StageResult;

#[derive(Debug, Clone, Copy)]
pub enum TimeSynchronizerConfig {
    ZeroOrderHold { fps: u32 },
    NearestNeighbor { fps: u32 },
}

impl TimeSynchronizerConfig {
    fn execute(self, frames: TopicFrames) -> StageResult<HashMap<String, LazyFrame>> {
        match (self, frames) {
            (TimeSynchronizerConfig::ZeroOrderHold { fps }, frames) => {
                ZeroOrderHoldTimeSynchronizer::new(fps).synchronize(frames)
            }
            (TimeSynchronizerConfig::NearestNeighbor { fps }, frames) => {
                NearestNeighborTimeSynchronizer::new(fps).synchronize(frames)
            }
        }
    }
}

pub fn synchronize(
    dataset: HashMap<String, LazyFrame>,
    config: TimeSynchronizerConfig,
) -> StageResult<HashMap<String, LazyFrame>> {
    config.execute(dataset)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;
    use std::io::Write;

    use camino::{Utf8Path, Utf8PathBuf};
    use polars::prelude::df;
    use tempfile::tempdir;

    use super::*;
    use crate::common::load_parquet_frames;
    use crate::synchronize::time_synchronizer::{IS_FRESH_COL, ORIGINAL_TIMESTAMP_COL};

    fn write_sample_parquet(path: &Utf8Path, timestamps: &[u64], values: &[i32]) {
        let mut dataframe = df!(
            ORIGINAL_TIMESTAMP_COL => timestamps,
            "value" => values,
        )
        .unwrap();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }

        let mut file = fs::File::create(path.as_std_path()).unwrap();
        ParquetWriter::new(&mut file)
            .finish(&mut dataframe)
            .unwrap();
        file.flush().unwrap();
    }

    fn assert_synched_parquet(frame: &LazyFrame, expected_rows: usize) {
        let dataframe = frame.clone().collect().unwrap();
        assert_eq!(dataframe.height(), expected_rows);

        let flags: Vec<bool> = dataframe
            .column(IS_FRESH_COL)
            .unwrap()
            .bool()
            .unwrap()
            .into_iter()
            .map(|v| v.unwrap())
            .collect();
        assert_eq!(flags.len(), expected_rows);
        assert!(flags.iter().any(|&flag| !flag));
    }

    #[test]
    fn synchronizes_directory_and_writes_outputs() {
        let temp_dir = tempdir().unwrap();
        let root = Utf8PathBuf::try_from(temp_dir.path().to_path_buf()).unwrap();
        let input_dir = root.join("sample_bag");
        let nested_dir = input_dir.join("nested");
        fs::create_dir_all(&nested_dir).unwrap();

        let topic1 = input_dir.join("topic1.parquet");
        let topic2 = nested_dir.join("topic2.parquet");

        write_sample_parquet(
            &topic1,
            &[
                200_000_000_u64,
                600_000_000,
                900_000_000,
                1_200_000_000,
                1_500_000_000,
            ],
            &[1_i32, 2, 3, 4, 5],
        );
        write_sample_parquet(
            &topic2,
            &[300_000_000_u64, 700_000_000, 950_000_000, 1_300_000_000],
            &[10_i32, 11, 12, 13],
        );

        let dataset = load_parquet_frames(&input_dir).unwrap();
        let synched =
            synchronize(dataset, TimeSynchronizerConfig::ZeroOrderHold { fps: 4 }).unwrap();

        let synched_topic1 = synched.get("/topic1").unwrap();
        let synched_topic2 = synched.get("/nested/topic2").unwrap();

        assert_synched_parquet(synched_topic1, 5);
        assert_synched_parquet(synched_topic2, 5);
    }

    #[test]
    fn synchronizes_ingested_mcap_dataset() {
        use crate::core::{Context, Stage};
        use crate::ingest::rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig};
        use crate::testutil::{McapGenerator, McapGeneratorConfig};

        // Generate synthetic MCAP data
        let temp_dir = tempdir().unwrap();
        let bag_dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf())
            .unwrap()
            .join("test_bag");

        let config = McapGeneratorConfig {
            num_frames: 50,
            fps: 10,
            joint_names: vec!["joint1".to_string(), "joint2".to_string()],
            image_size: (64, 64),
            base_frame: "base_link".to_string(),
            child_frames: vec!["arm_link".to_string()],
            generate_images: false,
            generate_tf: true,
            publish_time_offset_ns: 0,
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&bag_dir).unwrap();

        // Ingest the generated MCAP using Rosbag2Ingestor
        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);

        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let context = ingestor.run(context).expect("ingestor should succeed");
        let dataset = context.dataset.expect("dataset should exist");

        // Synchronize the dataset
        let synched = synchronize(
            dataset.clone(),
            TimeSynchronizerConfig::ZeroOrderHold { fps: 10 },
        )
        .unwrap();
        assert_eq!(synched.len(), dataset.len());

        // Verify output can be written to parquet
        let output_dir = Utf8PathBuf::from_path_buf(temp_dir.path().join("output")).unwrap();
        for (topic, frame) in synched {
            let output_path =
                output_dir.join(format!("{}.parquet", topic.strip_prefix("/").unwrap()));
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }

            let mut file = fs::File::create(output_path.as_std_path()).unwrap();

            ParquetWriter::new(&mut file)
                .finish(&mut frame.collect().unwrap())
                .unwrap();
        }
    }
}
