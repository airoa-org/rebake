use polars::prelude::*;
use serde::{Deserialize, Serialize};

use super::trajectory_utils::{
    build_joint_names_column, build_trajectory_point_struct, create_float64_list,
    default_time_series,
};
use crate::core::error::{OptionExt, PolarsExt, StageResult};
use crate::core::stage::{Context, Stage, StageConfig, StageError};

const HAND_TOPIC: &str = "/hsrb/gripper_controller/command";
const ARM_TOPIC: &str = "/hsrb/arm_trajectory_controller/command";
const SERVO_STATES_TOPIC: &str = "/hsrb/servo_states";
const HAND_JOINT_NAMES: [&str; 1] = ["hand_motor_joint"];

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandCommandEnricherConfig {}

impl HandCommandEnricherConfig {
    pub fn new() -> Self {
        Self::default()
    }
}

#[typetag::serde(name = "HandCommandEnricherConfig")]
impl StageConfig for HandCommandEnricherConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(HandCommandEnricher::new(self.clone()))
    }
}

/// An enricher that synthesizes hand gripper commands when the topic is missing from the rosbag.
///
/// This stage checks if the `/hsrb/gripper_controller/command` topic exists in the dataset.
/// If not present, it creates a synthetic topic by extracting the hand motor position from
/// `/hsrb/servo_states` and using the timestamp structure from `/hsrb/arm_trajectory_controller/command`.
///
/// This is useful for datasets where the gripper command topic was not recorded but the
/// gripper state was available through servo states.
///
/// # Preconditions
///
/// - `dataset`: **Required** (must contain `/hsrb/arm_trajectory_controller/command` and `/hsrb/servo_states` topics)
///
/// Note: If `/hsrb/gripper_controller/command` already exists, the stage returns early without modification.
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (with `/hsrb/gripper_controller/command` topic added if missing)
///
/// # Errors
///
/// - [`StageError::MissingData`]: `dataset` not set, `/hsrb/arm_trajectory_controller/command` missing,
///   or `/hsrb/servo_states` missing
/// - [`StageError::InvalidData`]: Required columns missing from source topics
/// - [`StageError::External`]: Failed to collect dataframe
pub struct HandCommandEnricher;

impl HandCommandEnricher {
    pub fn new(_config: HandCommandEnricherConfig) -> Self {
        Self
    }

    fn extract_hand_positions(servo_states: &LazyFrame) -> StageResult<f64> {
        let first_row = servo_states
            .clone()
            .select([col("present_position")])
            .limit(1)
            .collect()
            .map_err(|e| StageError::external("failed to collect servo states", e))?;

        let positions_series = first_row
            .column("present_position")
            .or_invalid("missing 'present_position' column in servo_states")?
            .list()
            .or_invalid("'present_position' column is not a list")?
            .get_as_series(0)
            .or_missing("first row in present_position list")?;

        let positions = positions_series.f64().or_invalid("positions are not f64")?;
        positions
            .get(6)
            .or_missing("hand_motor position at index 6")
    }

    /// Synthesizes trajectory points with the given hand motor position.
    ///
    /// # Errors
    ///
    /// Returns `StageError::InvalidData` if template_points elements are not structs,
    /// or `StageError::External` if trajectory point creation fails.
    fn synthesize_points_column(
        template_points: &ListChunked,
        hand_motor: f64,
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

            let positions = create_float64_list(&[hand_motor], "positions")?;
            let point = build_trajectory_point_struct(positions, time_series)?;
            results.push(point);
        }

        let mut series = Series::new("points".into(), results);
        series.rename("points".into());
        Ok(series)
    }

    fn build_hand_command_frame(arm_frame: &DataFrame, hand_motor: f64) -> StageResult<DataFrame> {
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

        let points = Self::synthesize_points_column(&template_points, hand_motor)?;
        let joint_names = build_joint_names_column(height, &HAND_JOINT_NAMES)?;

        DataFrame::new(vec![timestamps, header, joint_names.into(), points.into()])
            .map_err(|e| StageError::external("failed to create hand command DataFrame", e))
    }
}

impl Stage for HandCommandEnricher {
    fn name(&self) -> &'static str {
        "hand_command_enricher"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let dataset = context.dataset_mut().or_missing("dataset in context")?;

        if dataset.contains_key(HAND_TOPIC) {
            return Ok(context);
        }

        let arm_command = dataset
            .get(ARM_TOPIC)
            .ok_or_else(|| StageError::missing(format!("{} topic in dataset", ARM_TOPIC)))?;
        let servo_states = dataset.get(SERVO_STATES_TOPIC).ok_or_else(|| {
            StageError::missing(format!("{} topic in dataset", SERVO_STATES_TOPIC))
        })?;

        let hand_motor = Self::extract_hand_positions(servo_states)?;
        let arm_frame = arm_command
            .clone()
            .collect()
            .map_err(|e| StageError::external("failed to collect arm command dataframe", e))?;
        let synthesized = Self::build_hand_command_frame(&arm_frame, hand_motor)?;

        dataset.insert(HAND_TOPIC.to_string(), synthesized.lazy());
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
    fn enriches_hand_command_when_missing() {
        let mut dataset = ingest_dataset_clone();
        let arm_frame = dataset.get(ARM_TOPIC).unwrap().clone().collect().unwrap();

        let servo_states = dataset.get(SERVO_STATES_TOPIC).unwrap().clone();
        let hand_motor = HandCommandEnricher::extract_hand_positions(&servo_states).unwrap();

        dataset.remove(HAND_TOPIC);
        let mut context = Context::new(dataset);

        let mut enricher = HandCommandEnricherConfig::new().build();
        context = enricher.run(context).unwrap();

        let dataset = context.dataset.unwrap();
        let hand_frame = dataset.get(HAND_TOPIC).unwrap().clone().collect().unwrap();

        assert_eq!(hand_frame.height(), arm_frame.height());

        let synthesized_timestamps = hand_frame
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

        let joint_names_series = hand_frame
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
        assert_eq!(joint_names, HAND_JOINT_NAMES);

        let point_struct = hand_frame
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
        assert_eq!(positions, vec![hand_motor]);

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
        dataset.remove(HAND_TOPIC);

        let mut context = Context::new(dataset);
        let mut enricher = HandCommandEnricherConfig::new().build();
        context = enricher.run(context).unwrap();

        let first_run = context
            .dataset
            .as_ref()
            .unwrap()
            .get(HAND_TOPIC)
            .unwrap()
            .clone()
            .collect()
            .unwrap();

        let context = enricher.run(context).unwrap();
        let second_run = context
            .dataset
            .as_ref()
            .unwrap()
            .get(HAND_TOPIC)
            .unwrap()
            .clone()
            .collect()
            .unwrap();

        polars_testing::assert_dataframe_equal!(&first_run, &second_run);
    }
}
