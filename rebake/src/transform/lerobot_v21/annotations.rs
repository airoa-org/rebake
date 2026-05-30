//! Annotation builders for LeRobot v2.1 episodes.
//!
//! This module provides functions to build annotation DataFrames for episodes,
//! including task indices, done flags, and success indicators.

use polars::prelude::*;

use crate::core::StageError;
use crate::schema::metadata::v2_0::Segment;

/// Build annotations for SHT mode.
/// In SHT mode, `next.done` is true only for the last frame of each segment.
pub(crate) fn build_segment_annotations(
    segment: &Segment,
    row_count: usize,
    current_task_idx: i64,
    composite_task_idx: Option<i64>,
) -> Result<DataFrame, StageError> {
    if row_count == 0 {
        return Ok(df![
            "next.done" => Vec::<bool>::new(),
            "short_horizon_task_index" => Vec::<i64>::new(),
            "primitive_action_index" => Vec::<i64>::new(),
            "success_primitive_action" => Vec::<bool>::new(),
            "task_index" => Vec::<i64>::new(),
        ]?);
    }

    let mut done = vec![false; row_count];
    if let Some(last) = done.last_mut() {
        *last = true;
    }

    let composite_index = composite_task_idx.unwrap_or(-1);
    let short_horizon_task_index = vec![composite_index; row_count];
    let primitive_action_index = vec![current_task_idx; row_count];
    let success_primitive_action = vec![segment.success; row_count];
    let task_index = vec![current_task_idx; row_count];

    Ok(df![
        "next.done" => done,
        "short_horizon_task_index" => short_horizon_task_index,
        "primitive_action_index" => primitive_action_index,
        "success_primitive_action" => success_primitive_action,
        "task_index" => task_index,
    ]?)
}

/// Build annotations for PA mode.
/// In PA mode, each segment is a separate episode, so `next.done` is true only
/// for the last frame of the episode (which is also the last frame of the segment).
pub(crate) fn build_pa_segment_annotations(
    segment: &Segment,
    row_count: usize,
    current_task_idx: i64,
    composite_task_idx: Option<i64>,
) -> Result<DataFrame, StageError> {
    if row_count == 0 {
        return Ok(df![
            "next.done" => Vec::<bool>::new(),
            "short_horizon_task_index" => Vec::<i64>::new(),
            "primitive_action_index" => Vec::<i64>::new(),
            "success_primitive_action" => Vec::<bool>::new(),
            "task_index" => Vec::<i64>::new(),
        ]?);
    }

    // In PA mode, done is only true for the last frame of each episode
    let mut done = vec![false; row_count];
    if let Some(last) = done.last_mut() {
        *last = true;
    }

    let composite_index = composite_task_idx.unwrap_or(-1);
    let short_horizon_task_index = vec![composite_index; row_count];
    let primitive_action_index = vec![current_task_idx; row_count];
    let success_primitive_action = vec![segment.success; row_count];
    let task_index = vec![current_task_idx; row_count];

    Ok(df![
        "next.done" => done,
        "short_horizon_task_index" => short_horizon_task_index,
        "primitive_action_index" => primitive_action_index,
        "success_primitive_action" => success_primitive_action,
        "task_index" => task_index,
    ]?)
}
