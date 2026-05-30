use polars::prelude::*;
use serde::{Deserialize, Serialize};

use super::trajectory_utils::{
    build_joint_names_column, build_trajectory_point_struct, create_float64_list,
    default_time_series,
};
use crate::core::error::{OptionExt, PolarsExt, StageResult};
use crate::core::stage::{Context, Stage, StageConfig, StageError};

const HEAD_TOPIC: &str = "/hsrb/head_trajectory_controller/command";
const ARM_TOPIC: &str = "/hsrb/arm_trajectory_controller/command";
const JOINT_STATES_TOPIC: &str = "/hsrb/joint_states";
const HEAD_JOINT_NAMES: [&str; 2] = ["head_pan_joint", "head_tilt_joint"];

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeadCommandEnricherConfig {}

impl HeadCommandEnricherConfig {
    pub fn new() -> Self {
        Self::default()
    }
}

#[typetag::serde(name = "HeadCommandEnricherConfig")]
impl StageConfig for HeadCommandEnricherConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(HeadCommandEnricher::new(self.clone()))
    }
}

/// An enricher that synthesizes head trajectory commands when the topic is missing from the rosbag.
///
/// This stage checks if the `/hsrb/head_trajectory_controller/command` topic exists in the dataset.
/// If not present, it creates a synthetic topic by extracting the head pan and tilt positions from
/// `/hsrb/joint_states` and using the timestamp structure from `/hsrb/arm_trajectory_controller/command`.
///
/// This is useful for datasets where the head command topic was not recorded but the
/// head joint state was available through joint states.
///
/// # Preconditions
///
/// - `dataset`: **Required** (must contain `/hsrb/arm_trajectory_controller/command` and `/hsrb/joint_states` topics)
///
/// Note: If `/hsrb/head_trajectory_controller/command` already exists, the stage returns early without modification.
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (with `/hsrb/head_trajectory_controller/command` topic added if missing)
///
/// # Errors
///
/// - [`StageError::MissingData`]: `dataset` not set, `/hsrb/arm_trajectory_controller/command` missing,
///   or `/hsrb/joint_states` missing
/// - [`StageError::InvalidData`]: Required columns missing from source topics
/// - [`StageError::External`]: Failed to collect dataframe
pub struct HeadCommandEnricher;

impl HeadCommandEnricher {
    pub fn new(_config: HeadCommandEnricherConfig) -> Self {
        Self
    }

    fn extract_head_positions(joint_states: &LazyFrame) -> StageResult<(f64, f64)> {
        let first_row = joint_states
            .clone()
            .select([col("position")])
            .limit(1)
            .collect()
            .map_err(|e| StageError::external("failed to collect joint states", e))?;

        let positions_series = first_row
            .column("position")
            .or_invalid("missing 'position' column in joint_states")?
            .list()
            .or_invalid("'position' column is not a list")?
            .get_as_series(0)
            .or_missing("first row in position list")?;

        let positions = positions_series.f64().or_invalid("positions are not f64")?;
        let head_pan = positions
            .get(9)
            .or_missing("head_pan position at index 9")?;
        let head_tilt = positions
            .get(10)
            .or_missing("head_tilt position at index 10")?;
        Ok((head_pan, head_tilt))
    }

    /// Synthesizes trajectory points with the given head positions.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if template_points elements are not structs,
    /// or `StageError::External` if trajectory point creation fails.
    fn synthesize_points_column(
        template_points: &ListChunked,
        head_pan: f64,
        head_tilt: f64,
    ) -> StageResult<Series> {
        let mut results: Vec<Series> = Vec::with_capacity(template_points.len());

        for i in 0..template_points.len() {
            let original = template_points.get_as_series(i);
            let time_series = if let Some(series) = original {
                let struct_chunked = series.struct_().or_invalid(
                    "template_points elements must be struct - invalid trajectory schema",
                )?;
                struct_chunked
                    .field_by_name("time_from_start")
                    .ok()
                    .map(Ok)
                    .unwrap_or_else(default_time_series)?
            } else {
                default_time_series()?
            };

            let positions = create_float64_list(&[head_pan, head_tilt], "positions")?;
            let point = build_trajectory_point_struct(positions, time_series)?;
            results.push(point);
        }

        let mut series = Series::new("points".into(), results);
        series.rename("points".into());
        Ok(series)
    }

    fn build_head_command_frame(
        arm_frame: &DataFrame,
        head_pan: f64,
        head_tilt: f64,
    ) -> StageResult<DataFrame> {
        let height = arm_frame.height();
        let timestamps = arm_frame
            .column("timestamp_ns")
            .or_invalid("missing 'timestamp_ns' column in arm_frame")?
            .clone();
        let header = arm_frame
            .column("header")
            .or_invalid("missing 'header' column in arm_frame")?
            .clone();
        let template_points = arm_frame
            .column("points")
            .or_invalid("missing 'points' column in arm_frame")?
            .list()
            .or_invalid("'points' column is not a list")?
            .clone();

        let points = Self::synthesize_points_column(&template_points, head_pan, head_tilt)?;
        let joint_names = build_joint_names_column(height, &HEAD_JOINT_NAMES)?;

        DataFrame::new(vec![timestamps, header, joint_names.into(), points.into()])
            .map_err(|e| StageError::external("failed to create head command DataFrame", e))
    }
}

impl Stage for HeadCommandEnricher {
    fn name(&self) -> &'static str {
        "head_command_enricher"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let dataset = context.dataset_mut().or_missing("dataset in context")?;

        if dataset.contains_key(HEAD_TOPIC) {
            return Ok(context);
        }

        let arm_command = dataset
            .get(ARM_TOPIC)
            .ok_or_else(|| StageError::missing(format!("{} topic in dataset", ARM_TOPIC)))?;
        let joint_states = dataset.get(JOINT_STATES_TOPIC).ok_or_else(|| {
            StageError::missing(format!("{} topic in dataset", JOINT_STATES_TOPIC))
        })?;

        let (head_pan, head_tilt) = Self::extract_head_positions(joint_states)?;
        let arm_frame = arm_command
            .clone()
            .collect()
            .map_err(|e| StageError::external("failed to collect arm command dataframe", e))?;
        let synthesized = Self::build_head_command_frame(&arm_frame, head_pan, head_tilt)?;

        dataset.insert(HEAD_TOPIC.to_string(), synthesized.lazy());
        Ok(context)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::test_utils::ingest_dataset_clone;

    #[test]
    #[ignore = "requires external fixture file"]
    fn enriches_head_command_when_missing() {
        let mut dataset = ingest_dataset_clone();
        let arm_frame = dataset.get(ARM_TOPIC).unwrap().clone().collect().unwrap();

        let joint_states = dataset.get(JOINT_STATES_TOPIC).unwrap().clone();
        let (head_pan, head_tilt) =
            HeadCommandEnricher::extract_head_positions(&joint_states).unwrap();

        dataset.remove(HEAD_TOPIC);
        let mut context = Context::new(dataset);

        let mut enricher = HeadCommandEnricherConfig::new().build();
        context = enricher.run(context).unwrap();

        let dataset = context.dataset.unwrap();
        let head_frame = dataset.get(HEAD_TOPIC).unwrap().clone().collect().unwrap();

        assert_eq!(head_frame.height(), arm_frame.height());

        let synthesized_timestamps = head_frame
            .column("timestamp_ns")
            .unwrap()
            .as_series()
            .unwrap()
            .clone();
        let expected_timestamps = arm_frame
            .column("timestamp_ns")
            .unwrap()
            .as_series()
            .unwrap()
            .clone();
        polars_testing::assert_series_equal!(&synthesized_timestamps, &expected_timestamps);

        let joint_names_series = head_frame
            .column("joint_names")
            .unwrap()
            .list()
            .unwrap()
            .get_as_series(0)
            .unwrap()
            .clone();
        let joint_names = joint_names_series
            .str()
            .unwrap()
            .into_no_null_iter()
            .collect::<Vec<_>>();
        assert_eq!(joint_names, HEAD_JOINT_NAMES);

        let point_struct = head_frame
            .column("points")
            .unwrap()
            .list()
            .unwrap()
            .get_as_series(0)
            .unwrap()
            .struct_()
            .unwrap()
            .clone();

        let positions = point_struct
            .field_by_name("positions")
            .unwrap()
            .list()
            .unwrap()
            .get_as_series(0)
            .unwrap()
            .f64()
            .unwrap()
            .into_no_null_iter()
            .collect::<Vec<_>>();
        assert_eq!(positions, vec![head_pan, head_tilt]);

        let velocities = point_struct
            .field_by_name("velocities")
            .unwrap()
            .list()
            .unwrap()
            .get_as_series(0)
            .unwrap()
            .len();
        assert_eq!(velocities, 0);

        let accelerations = point_struct
            .field_by_name("accelerations")
            .unwrap()
            .list()
            .unwrap()
            .get_as_series(0)
            .unwrap()
            .len();
        assert_eq!(accelerations, 0);

        let effort = point_struct
            .field_by_name("effort")
            .unwrap()
            .list()
            .unwrap()
            .get_as_series(0)
            .unwrap()
            .len();
        assert_eq!(effort, 0);

        let synthesized_time = point_struct.field_by_name("time_from_start").unwrap();
        let original_time = arm_frame
            .column("points")
            .unwrap()
            .list()
            .unwrap()
            .get_as_series(0)
            .unwrap()
            .struct_()
            .unwrap()
            .clone()
            .field_by_name("time_from_start")
            .unwrap();
        polars_testing::assert_series_equal!(&synthesized_time, &original_time);
    }

    #[test]
    #[ignore = "requires external fixture file"]
    fn enricher_is_idempotent() {
        let mut dataset = ingest_dataset_clone();
        dataset.remove(HEAD_TOPIC);

        let mut context = Context::new(dataset);
        let mut enricher = HeadCommandEnricherConfig::new().build();
        context = enricher.run(context).unwrap();

        let first_run = context
            .dataset
            .as_ref()
            .unwrap()
            .get(HEAD_TOPIC)
            .unwrap()
            .clone()
            .collect()
            .unwrap();

        let context = enricher.run(context).unwrap();
        let second_run = context
            .dataset
            .as_ref()
            .unwrap()
            .get(HEAD_TOPIC)
            .unwrap()
            .clone()
            .collect()
            .unwrap();

        polars_testing::assert_dataframe_equal!(&first_run, &second_run);
    }
}
