use polars::chunked_array::builder::get_list_builder;
use polars::prelude::*;

use crate::core::error::{ResultExt, StageResult};

/// Creates a default `time_from_start` struct series with zero values.
///
/// This is used when synthesizing trajectory command messages that don't have
/// a `time_from_start` field in the template.
///
/// # Errors
///
/// Returns `StageError::External` if the struct construction fails.
pub fn default_time_series() -> StageResult<Series> {
    Ok(StructChunked::from_series(
        "time_from_start".into(),
        1,
        [
            Series::new("sec".into(), &[0i32]),
            Series::new("nanosec".into(), &[0u32]),
        ]
        .iter(),
    )
    .with_context("failed to create default time_from_start struct")?
    .into_series())
}

/// Creates an empty list of f64 values with the given name.
///
/// Used for creating empty `velocities`, `accelerations`, and `effort` fields
/// in synthesized trajectory points.
///
/// # Errors
///
/// Returns `StageError::External` if appending to the list builder fails.
pub fn empty_float_list(name: &str) -> StageResult<Series> {
    let mut builder = get_list_builder(&DataType::Float64, 1, 0, name.into());
    builder
        .append_series(&Series::new(PlSmallStr::EMPTY, &[] as &[f64]))
        .with_context("failed to append empty f64 series to list builder")?;
    Ok(builder.finish().into_series())
}

/// Builds a column of repeated joint names lists.
///
/// Each row in the output contains the same list of joint names.
///
/// # Errors
///
/// Returns `StageError::External` if appending to the list builder fails.
pub fn build_joint_names_column(height: usize, joint_names: &[&str]) -> StageResult<Series> {
    let mut builder = get_list_builder(
        &DataType::String,
        height,
        joint_names.len(),
        "joint_names".into(),
    );
    let names = Series::new(PlSmallStr::EMPTY, joint_names);
    for _ in 0..height {
        builder
            .append_series(&names)
            .with_context("failed to append joint names to list builder")?;
    }
    Ok(builder.finish().into_series())
}

/// Creates a list of f64 values with the given name.
///
/// Used for creating `positions` fields in synthesized trajectory points.
///
/// # Errors
///
/// Returns `StageError::External` if appending to the list builder fails.
pub fn create_float64_list(values: &[f64], name: &str) -> StageResult<Series> {
    let mut builder = get_list_builder(&DataType::Float64, 1, values.len(), name.into());
    builder
        .append_series(&Series::new(PlSmallStr::EMPTY, values))
        .with_context("failed to append f64 values to list builder")?;
    Ok(builder.finish().into_series())
}

/// Builds a trajectory point struct series with positions and time_from_start.
///
/// Creates a struct with: positions, velocities (empty), accelerations (empty),
/// effort (empty), and time_from_start.
///
/// # Errors
///
/// Returns `StageError::External` if creating the empty lists or the struct fails.
pub fn build_trajectory_point_struct(
    positions: Series,
    time_from_start: Series,
) -> StageResult<Series> {
    let velocities = empty_float_list("velocities")?;
    let accelerations = empty_float_list("accelerations")?;
    let effort = empty_float_list("effort")?;

    let mut time_series = time_from_start;
    time_series.rename("time_from_start".into());

    Ok(StructChunked::from_series(
        "".into(),
        1,
        [positions, velocities, accelerations, effort, time_series].iter(),
    )
    .with_context("failed to create trajectory point struct")?
    .into_series())
}
