use polars::chunked_array::ops::FillNullStrategy;
use polars::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::core::error::OptionExt;
use crate::core::stage::{Context, Stage, StageConfig, StageError};
use crate::synchronize::{IS_FRESH_COL, ORIGINAL_TIMESTAMP_COL, SYNCHED_TIMESTAMP_COL};

/// Columns excluded from shifting (time metadata managed by the synchronize stage).
const EXCLUDED_COLUMNS: &[&str] = &[SYNCHED_TIMESTAMP_COL, ORIGINAL_TIMESTAMP_COL, IS_FRESH_COL];

/// Strategy for filling null values introduced by shifting.
///
/// When columns are shifted, the rows at the boundary (start or end, depending
/// on shift direction) become null. This enum controls how those nulls are filled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FillStrategy {
    /// Fill nulls with the nearest edge value (forward fill then backward fill).
    ///
    /// Works for all column types. This is the safest default for robotics data
    /// where the last known state is a reasonable fill value.
    #[default]
    Edge,

    /// Fill nulls with zero for numeric scalar types; falls back to `Edge` for others.
    ///
    /// Numeric scalar types: f64, f32, i64, i32, i16, i8, u64, u32, u16, u8.
    /// Non-numeric types (String, List, Struct, etc.) use `Edge` strategy.
    Zero,
}

/// Configuration for the [`ShiftEnricher`] stage.
///
/// Creates a new topic by shifting the source topic's column values by a specified
/// number of steps. The source topic is preserved unchanged, making it possible to
/// use both state (original) and action (shifted) data simultaneously for VLA model training.
///
/// Each config handles a single source-to-output pair. Use multiple `ShiftEnricherConfig`
/// entries in the pipeline to shift multiple topics.
///
/// # Example YAML
///
/// ```yaml
/// - ShiftEnricherConfig:
///     source_topic: "/joint_states"
///     output_topic: "/joint_states/action"
///     shift_steps: 1
///     fill_strategy: edge
/// - ShiftEnricherConfig:
///     source_topic: "/tf_chain"
///     output_topic: "/tf_chain/action"
///     shift_steps: 1
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShiftEnricherConfig {
    /// Source topic to read data from. This topic is not modified.
    pub source_topic: String,

    /// Output topic name for the shifted data.
    pub output_topic: String,

    /// Number of steps to shift. Positive = future direction, negative = past direction.
    ///
    /// For VLA training, use `1` so each row's value is replaced by the next step's value,
    /// making it suitable as an action label.
    pub shift_steps: i64,

    /// Strategy for filling null values created by shifting.
    #[serde(default)]
    pub fill_strategy: FillStrategy,
}

impl ShiftEnricherConfig {
    pub fn new(
        source_topic: impl Into<String>,
        output_topic: impl Into<String>,
        shift_steps: i64,
    ) -> Self {
        Self {
            source_topic: source_topic.into(),
            output_topic: output_topic.into(),
            shift_steps,
            fill_strategy: FillStrategy::default(),
        }
    }

    pub fn with_fill_strategy(mut self, fill_strategy: FillStrategy) -> Self {
        self.fill_strategy = fill_strategy;
        self
    }
}

#[typetag::serde(name = "ShiftEnricherConfig")]
impl StageConfig for ShiftEnricherConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(ShiftEnricher::new(self.clone()))
    }
}

/// An enricher that creates a new topic with shifted column values.
///
/// Reads the source topic, shifts all data columns (excluding time metadata columns:
/// `synched_timestamp_ns`, `timestamp_ns`, `is_fresh`) by the specified number of steps,
/// and inserts the result as a new topic. The source topic is preserved unchanged.
///
/// This is primarily used for VLA model training where actions correspond to future
/// observations (e.g., `shift_steps = 1` creates a topic where each row contains
/// the next step's value).
///
/// # Preconditions
///
/// - `dataset`: **Required** (must contain `source_topic`)
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (`output_topic` added with shifted column values,
///   `source_topic` unchanged)
///
/// Note: If `source_topic` is not found, the stage is silently skipped.
/// A `shift_steps` of 0 is a no-op (early return).
///
/// # Errors
///
/// - [`StageError::MissingData`]: `dataset` not set in context
pub struct ShiftEnricher {
    source_topic: String,
    output_topic: String,
    shift_steps: i64,
    fill_strategy: FillStrategy,
}

impl ShiftEnricher {
    pub fn new(config: ShiftEnricherConfig) -> Self {
        Self {
            source_topic: config.source_topic,
            output_topic: config.output_topic,
            shift_steps: config.shift_steps,
            fill_strategy: config.fill_strategy,
        }
    }
}

impl Stage for ShiftEnricher {
    fn name(&self) -> &'static str {
        "shift_enricher"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        if self.shift_steps == 0 {
            debug!("shift_steps is 0, skipping");
            return Ok(context);
        }

        let dataset = context.dataset_mut().or_missing("dataset in context")?;

        let Some(source_frame) = dataset.get(&self.source_topic) else {
            debug!(
                topic = self.source_topic,
                "source topic not found, skipping"
            );
            return Ok(context);
        };

        let schema = source_frame.clone().collect_schema()?;

        let shift_exprs: Vec<Expr> = schema
            .iter()
            .filter(|(name, _)| !EXCLUDED_COLUMNS.contains(&name.as_str()))
            .map(|(name, dtype)| {
                build_shift_fill_expr(name, dtype, self.shift_steps, &self.fill_strategy)
            })
            .collect();

        let shifted_frame = if shift_exprs.is_empty() {
            source_frame.clone()
        } else {
            source_frame.clone().with_columns(shift_exprs)
        };

        dataset.insert(self.output_topic.clone(), shifted_frame);

        Ok(context)
    }
}

/// Build a shift + fill expression for a single column.
///
/// The sign of `shift_steps` is negated here to translate the user-facing convention
/// (positive = future) into Polars' internal convention (positive = shift down = past).
fn build_shift_fill_expr(
    name: &str,
    dtype: &DataType,
    shift_steps: i64,
    fill_strategy: &FillStrategy,
) -> Expr {
    let shifted = col(name).shift(lit(-shift_steps));

    match fill_strategy {
        FillStrategy::Edge => apply_edge_fill(shifted, name),
        FillStrategy::Zero => {
            if is_numeric_scalar(dtype) {
                shifted.fill_null(lit(0)).alias(name)
            } else {
                // Non-numeric and compound types (List, Struct, String, etc.)
                // fall back to Edge strategy.
                apply_edge_fill(shifted, name)
            }
        }
    }
}

/// Apply forward fill then backward fill to handle nulls at both ends.
///
/// - Positive shift (future): nulls appear at the end -> forward fill covers them
/// - Negative shift (past): nulls appear at the start -> backward fill covers them
/// - Using both ensures correct fill regardless of shift direction.
fn apply_edge_fill(expr: Expr, name: &str) -> Expr {
    expr.fill_null_with_strategy(FillNullStrategy::Forward(None))
        .fill_null_with_strategy(FillNullStrategy::Backward(None))
        .alias(name)
}

/// Check if a DataType is a scalar numeric type.
fn is_numeric_scalar(dtype: &DataType) -> bool {
    matches!(
        dtype,
        DataType::Float64
            | DataType::Float32
            | DataType::Int64
            | DataType::Int32
            | DataType::Int16
            | DataType::Int8
            | DataType::UInt64
            | DataType::UInt32
            | DataType::UInt16
            | DataType::UInt8
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn make_context(topic: &str, df: DataFrame) -> Context {
        let mut dataset = HashMap::new();
        dataset.insert(topic.to_string(), df.lazy());
        Context::new(dataset)
    }

    fn collect_f64(df: &DataFrame, col_name: &str) -> Vec<f64> {
        df.column(col_name)
            .unwrap()
            .f64()
            .unwrap()
            .into_no_null_iter()
            .collect()
    }

    #[test]
    fn creates_output_topic_and_preserves_source() {
        let df = df!(
            "synched_timestamp_ns" => &[1u64, 2, 3, 4, 5],
            "value" => &[10.0f64, 20.0, 30.0, 40.0, 50.0]
        )
        .unwrap();

        let context = make_context("/state", df);
        let config = ShiftEnricherConfig::new("/state", "/action", 1);
        let mut stage = ShiftEnricher::new(config);
        let result = stage.run(context).unwrap();

        let dataset = result.dataset().unwrap();

        // Source topic is preserved unchanged
        let source_df = dataset.get("/state").unwrap().clone().collect().unwrap();
        assert_eq!(
            collect_f64(&source_df, "value"),
            vec![10.0, 20.0, 30.0, 40.0, 50.0]
        );

        // Output topic has shifted values
        let output_df = dataset.get("/action").unwrap().clone().collect().unwrap();
        // shift_steps=1 (future): [20, 30, 40, 50, null] -> edge fill -> [20, 30, 40, 50, 50]
        assert_eq!(
            collect_f64(&output_df, "value"),
            vec![20.0, 30.0, 40.0, 50.0, 50.0]
        );
    }

    #[test]
    fn past_shift_with_edge_fill() {
        let df = df!(
            "synched_timestamp_ns" => &[1u64, 2, 3, 4, 5],
            "value" => &[10.0f64, 20.0, 30.0, 40.0, 50.0]
        )
        .unwrap();

        let context = make_context("/state", df);
        let config = ShiftEnricherConfig::new("/state", "/past", -1);
        let mut stage = ShiftEnricher::new(config);
        let result = stage.run(context).unwrap();

        let dataset = result.dataset().unwrap();
        let output_df = dataset.get("/past").unwrap().clone().collect().unwrap();

        // shift_steps=-1 (past): [null, 10, 20, 30, 40] -> edge fill -> [10, 10, 20, 30, 40]
        assert_eq!(
            collect_f64(&output_df, "value"),
            vec![10.0, 10.0, 20.0, 30.0, 40.0]
        );
    }

    #[test]
    fn zero_fill_with_numeric_type() {
        let df = df!(
            "synched_timestamp_ns" => &[1u64, 2, 3, 4, 5],
            "value" => &[10.0f64, 20.0, 30.0, 40.0, 50.0]
        )
        .unwrap();

        let context = make_context("/state", df);
        let config =
            ShiftEnricherConfig::new("/state", "/action", 1).with_fill_strategy(FillStrategy::Zero);
        let mut stage = ShiftEnricher::new(config);
        let result = stage.run(context).unwrap();

        let dataset = result.dataset().unwrap();
        let output_df = dataset.get("/action").unwrap().clone().collect().unwrap();

        // shift_steps=1 (future): [20, 30, 40, 50, null] -> fill_null(0) -> [20, 30, 40, 50, 0]
        assert_eq!(
            collect_f64(&output_df, "value"),
            vec![20.0, 30.0, 40.0, 50.0, 0.0]
        );
    }

    #[test]
    fn zero_fill_falls_back_to_edge_for_non_numeric() {
        let df = df!(
            "synched_timestamp_ns" => &[1u64, 2, 3],
            "label" => &["a", "b", "c"]
        )
        .unwrap();

        let context = make_context("/state", df);
        let config =
            ShiftEnricherConfig::new("/state", "/action", 1).with_fill_strategy(FillStrategy::Zero);
        let mut stage = ShiftEnricher::new(config);
        let result = stage.run(context).unwrap();

        let dataset = result.dataset().unwrap();
        let output_df = dataset.get("/action").unwrap().clone().collect().unwrap();

        // shift_steps=1 (future): ["b", "c", null] -> edge fill -> ["b", "c", "c"]
        let values: Vec<&str> = output_df
            .column("label")
            .unwrap()
            .str()
            .unwrap()
            .into_no_null_iter()
            .collect();
        assert_eq!(values, vec!["b", "c", "c"]);
    }

    #[test]
    fn excluded_columns_unchanged_in_output() {
        let df = df!(
            "synched_timestamp_ns" => &[100u64, 200, 300],
            "timestamp_ns" => &[100u64, 200, 300],
            "is_fresh" => &[true, false, true],
            "value" => &[1.0f64, 2.0, 3.0]
        )
        .unwrap();

        let context = make_context("/state", df);
        let config = ShiftEnricherConfig::new("/state", "/action", 1);
        let mut stage = ShiftEnricher::new(config);
        let result = stage.run(context).unwrap();

        let dataset = result.dataset().unwrap();
        let output_df = dataset.get("/action").unwrap().clone().collect().unwrap();

        // Time metadata columns should be preserved from source (not shifted)
        let ts: Vec<u64> = output_df
            .column("synched_timestamp_ns")
            .unwrap()
            .u64()
            .unwrap()
            .into_no_null_iter()
            .collect();
        assert_eq!(ts, vec![100, 200, 300]);

        let orig_ts: Vec<u64> = output_df
            .column("timestamp_ns")
            .unwrap()
            .u64()
            .unwrap()
            .into_no_null_iter()
            .collect();
        assert_eq!(orig_ts, vec![100, 200, 300]);

        let fresh: Vec<bool> = output_df
            .column("is_fresh")
            .unwrap()
            .bool()
            .unwrap()
            .into_no_null_iter()
            .collect();
        assert_eq!(fresh, vec![true, false, true]);

        // Data column should be shifted
        assert_eq!(collect_f64(&output_df, "value"), vec![2.0, 3.0, 3.0]);
    }

    #[test]
    fn shift_zero_is_noop() {
        let df = df!(
            "synched_timestamp_ns" => &[1u64, 2, 3],
            "value" => &[10.0f64, 20.0, 30.0]
        )
        .unwrap();

        let context = make_context("/state", df);
        let config = ShiftEnricherConfig::new("/state", "/action", 0);
        let mut stage = ShiftEnricher::new(config);
        let result = stage.run(context).unwrap();

        let dataset = result.dataset().unwrap();
        // Output topic should not be created
        assert!(dataset.get("/action").is_none());
        // Source should remain unchanged
        let source_df = dataset.get("/state").unwrap().clone().collect().unwrap();
        assert_eq!(collect_f64(&source_df, "value"), vec![10.0, 20.0, 30.0]);
    }

    #[test]
    fn missing_source_topic_skipped() {
        let df = df!(
            "synched_timestamp_ns" => &[1u64, 2, 3],
            "value" => &[10.0f64, 20.0, 30.0]
        )
        .unwrap();

        let context = make_context("/state", df);
        let config = ShiftEnricherConfig::new("/nonexistent", "/action", 1);
        let mut stage = ShiftEnricher::new(config);
        let result = stage.run(context).unwrap();

        let dataset = result.dataset().unwrap();
        // Output topic should not be created
        assert!(dataset.get("/action").is_none());
        // Existing topic should be unchanged
        let source_df = dataset.get("/state").unwrap().clone().collect().unwrap();
        assert_eq!(collect_f64(&source_df, "value"), vec![10.0, 20.0, 30.0]);
    }

    #[test]
    fn mixed_column_types() {
        let df = df!(
            "synched_timestamp_ns" => &[1u64, 2, 3],
            "float_val" => &[1.0f64, 2.0, 3.0],
            "int_val" => &[10i32, 20, 30]
        )
        .unwrap();

        let context = make_context("/state", df);
        let config = ShiftEnricherConfig::new("/state", "/action", 1);
        let mut stage = ShiftEnricher::new(config);
        let result = stage.run(context).unwrap();

        let dataset = result.dataset().unwrap();
        let output_df = dataset.get("/action").unwrap().clone().collect().unwrap();

        // Float column: shift_steps=1 (future) + edge fill -> [2.0, 3.0, 3.0]
        assert_eq!(collect_f64(&output_df, "float_val"), vec![2.0, 3.0, 3.0]);

        // Int column: shift_steps=1 (future) + edge fill -> [20, 30, 30]
        let int_vals: Vec<i32> = output_df
            .column("int_val")
            .unwrap()
            .i32()
            .unwrap()
            .into_no_null_iter()
            .collect();
        assert_eq!(int_vals, vec![20, 30, 30]);
    }
}
