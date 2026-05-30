//! Segment assembly for LeRobot episodes.
//!
//! # Overview
//!
//! Assembles episode data from synchronized topic LazyFrames based on
//! segment definitions in Airoa metadata. Handles feature extraction,
//! joining, and struct-to-list normalization.
//!
//! # Responsibilities
//!
//! - Owns: Segment time filtering, feature extraction, LazyFrame joining
//! - Does not own: Video encoding (see [`super::video`])

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;

use polars::prelude::*;

use crate::core::{StageError, StageResult};
use crate::schema::metadata::v2_0::Segment;
use crate::schema::{TopicFeatureMap, TopicFeatureMapEntry};
use crate::synchronize::time_synchronizer::SYNCHED_TIMESTAMP_COL;

mod field_selector;

pub struct SegmentAssembler<'a> {
    feature_map: &'a TopicFeatureMap,
}

impl<'a> SegmentAssembler<'a> {
    pub fn new(feature_map: &'a TopicFeatureMap) -> Self {
        Self { feature_map }
    }

    pub fn assemble(
        &self,
        dataset: &HashMap<String, LazyFrame>,
        segment: &Segment,
    ) -> Result<DataFrame, Box<dyn Error + Send + Sync>> {
        // 1. Extract: Create LazyFrames for each topic with selected features
        let segment_frames = Self::extract_segment_data(dataset, segment);
        let feature_frames = Self::create_feature_lazyframes(self.feature_map, &segment_frames)?;

        // 2. Join: Merge all feature frames into a single DataFrame
        let joined_frame = Self::join_all_features(feature_frames)?;

        // 3. Materialize & Normalize: Collect and convert structs to lists
        let df = joined_frame.collect()?;
        let normalized_df = normalize_structs_to_lists(df)?;

        // 4. Validate: Ensure feature lengths match expected dimensions
        self.validate_feature_lengths(&normalized_df)?;

        Ok(normalized_df)
    }

    fn create_feature_lazyframes(
        feature_map: &TopicFeatureMap,
        dataset: &HashMap<String, LazyFrame>,
    ) -> Result<Vec<LazyFrame>, Box<dyn Error + Send + Sync>> {
        validate_required_topics(feature_map, dataset)?;

        let mut frames = Vec::new();
        // Use BTreeMap for automatic sorting by topic to ensure deterministic output
        let mut groups: BTreeMap<&str, Vec<&TopicFeatureMapEntry>> = BTreeMap::new();

        for entry in &feature_map.map {
            groups.entry(entry.topic()).or_default().push(entry);
        }

        for (topic, mut entries) in groups {
            let lf = dataset.get(topic).ok_or_else(|| {
                StageError::invalid(format!(
                    "required topic '{}' is missing from dataset after validation",
                    topic
                ))
            })?;
            let mut exprs = vec![col(SYNCHED_TIMESTAMP_COL)];

            // Sort entries within each topic by feature name for deterministic order
            entries.sort_by_key(|e| e.feature());

            for entry in entries {
                let expr = match entry {
                    TopicFeatureMapEntry::Parquet { field, feature, .. } => {
                        field_selector::build_field_expression(field)
                            .map_err(|err| format!("invalid field path '{field}': {err}"))?
                            .alias(feature)
                    }
                    TopicFeatureMapEntry::Video { feature, .. }
                    | TopicFeatureMapEntry::Image { feature, .. } => {
                        col("index").alias(format!("image_index_{}", feature))
                    }
                };
                exprs.push(expr);
            }

            frames.push(lf.clone().select(&exprs));
        }

        if frames.is_empty() {
            return Err("No matching topic data found in the input directory".into());
        }

        Ok(frames)
    }

    fn validate_feature_lengths(&self, df: &DataFrame) -> Result<(), Box<dyn Error + Send + Sync>> {
        for entry in &self.feature_map.map {
            if let Some(names) = entry.names() {
                let feature = entry.feature();
                let expected_len = names.len();

                if let Ok(col) = df.column(feature) {
                    // Check if column is List
                    if let DataType::List(_) = col.dtype() {
                        let ca = col.list()?;
                        for (i, opt_s) in ca.into_iter().enumerate() {
                            if let Some(s) = opt_s
                                && s.len() != expected_len
                            {
                                return Err(format!(
                                        "Feature '{}' at row {} has length {}, expected {} (names: {:?})",
                                        feature, i, s.len(), expected_len, names
                                    )
                                    .into());
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn extract_segment_data(
        dataset: &HashMap<String, LazyFrame>,
        segment: &Segment,
    ) -> HashMap<String, LazyFrame> {
        let start_time = (segment.start_time * 1_000_000_000.0) as i64;
        let end_time = (segment.end_time * 1_000_000_000.0) as i64;

        dataset
            .iter()
            .map(|(path, lf)| {
                (
                    path.clone(),
                    lf.clone().filter(
                        col(SYNCHED_TIMESTAMP_COL)
                            .gt_eq(lit(start_time))
                            .and(col(SYNCHED_TIMESTAMP_COL).lt_eq(lit(end_time))),
                    ),
                )
            })
            .collect()
    }

    fn join_all_features(
        frames: Vec<LazyFrame>,
    ) -> Result<LazyFrame, Box<dyn Error + Send + Sync>> {
        frames
            .into_iter()
            .reduce(|left, right| {
                left.join(
                    right,
                    &[col(SYNCHED_TIMESTAMP_COL)],
                    &[col(SYNCHED_TIMESTAMP_COL)],
                    JoinArgs::new(JoinType::Inner),
                )
            })
            .ok_or_else(|| "No frames to join".into())
    }
}

/// Validate that all topics required by the robot model are present in the dataset.
///
/// `TopicFeatureMap` defines the feature contract for the resulting LeRobot dataset.
/// If any referenced topic is absent, we fail fast instead of silently shrinking
/// the output schema. This prevents producing incompatible datasets that only fail
/// later during merge.
pub(crate) fn validate_required_topics(
    feature_map: &TopicFeatureMap,
    dataset: &HashMap<String, LazyFrame>,
) -> StageResult<()> {
    let mut missing_topics: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for entry in &feature_map.map {
        if dataset.contains_key(entry.topic()) {
            continue;
        }

        missing_topics
            .entry(entry.topic().to_string())
            .or_default()
            .insert(entry.feature().to_string());
    }

    if missing_topics.is_empty() {
        return Ok(());
    }

    let details = missing_topics
        .into_iter()
        .map(|(topic, features)| {
            format!(
                "{} (features: {})",
                topic,
                features.into_iter().collect::<Vec<_>>().join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("; ");

    Err(StageError::invalid(format!(
        "robot_model requires topics that are missing from dataset: {}",
        details
    )))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use jsonptr::Pointer;
    use polars::{df, prelude::IntoLazy};

    #[test]
    fn parses_json_pointer_segments() {
        let pointer = Pointer::parse("/points/0/positions").unwrap();
        let segments: Vec<_> = pointer
            .tokens()
            .map(|token| token.decoded().into_owned())
            .collect();
        assert_eq!(segments, vec!["points", "0", "positions"]);
    }

    #[test]
    fn rejects_pointers_without_prefix() {
        assert!(Pointer::parse("points/0").is_err());
    }

    #[test]
    fn test_validate_feature_lengths() {
        let feature_map = TopicFeatureMap {
            map: vec![TopicFeatureMapEntry::Parquet {
                topic: "topic".to_string(),
                field: "/field".to_string(),
                feature: "feature".to_string(),
                names: Some(vec!["x".to_string(), "y".to_string()]),
                description: None,
            }],
        };
        let assembler = SegmentAssembler::new(&feature_map);

        // Valid case: length 2
        let _s = Series::new(
            "feature".into(),
            &[
                Series::new("item".into(), &[1, 2]),
                Series::new("item".into(), &[3, 4]),
            ],
        );
        // We need List Series
        let list_ca: ListChunked = vec![
            Some(Series::new("item".into(), &[1, 2])),
            Some(Series::new("item".into(), &[3, 4])),
        ]
        .into_iter()
        .collect();
        let s = list_ca.into_series().with_name("feature".into());

        let df = DataFrame::new(vec![s.into()]).unwrap();
        assert!(assembler.validate_feature_lengths(&df).is_ok());

        // Invalid case: length 1
        let list_ca_invalid: ListChunked = vec![
            Some(Series::new("item".into(), &[1])),
            Some(Series::new("item".into(), &[3])),
        ]
        .into_iter()
        .collect();
        let s_invalid = list_ca_invalid.into_series().with_name("feature".into());

        let df_invalid = DataFrame::new(vec![s_invalid.into()]).unwrap();
        assert!(assembler.validate_feature_lengths(&df_invalid).is_err());
    }

    #[test]
    fn validate_required_topics_reports_missing_topic_and_features() {
        let feature_map = TopicFeatureMap {
            map: vec![
                TopicFeatureMapEntry::Parquet {
                    topic: "/present".to_string(),
                    field: "/value".to_string(),
                    feature: "observation.value".to_string(),
                    names: None,
                    description: None,
                },
                TopicFeatureMapEntry::Parquet {
                    topic: "/missing".to_string(),
                    field: "/position".to_string(),
                    feature: "action.ee_joint_command".to_string(),
                    names: Some(vec!["right_hand_joint1".to_string()]),
                    description: None,
                },
                TopicFeatureMapEntry::Video {
                    topic: "/missing".to_string(),
                    feature: "observation.image.hand".to_string(),
                    names: None,
                    description: None,
                },
            ],
        };

        let dataset = HashMap::from([(
            "/present".to_string(),
            df! {
                SYNCHED_TIMESTAMP_COL => &[1_u64],
                "value" => &[1_i32],
            }
            .unwrap()
            .lazy(),
        )]);

        let err = validate_required_topics(&feature_map, &dataset).unwrap_err();
        let message = err.to_string();

        assert!(matches!(err, StageError::InvalidData { .. }));
        assert!(message.contains("/missing"));
        assert!(message.contains("action.ee_joint_command"));
        assert!(message.contains("observation.image.hand"));
    }

    #[test]
    fn filter_segments_with_indices_preserves_original_segment_index() {
        let segments = vec![
            Segment {
                start_time: 0.0,
                end_time: 1.0,
                label_idx: 0,
                success: true,
            },
            Segment {
                start_time: 2.0,
                end_time: 3.0,
                label_idx: 1,
                success: true,
            },
            Segment {
                start_time: 4.0,
                end_time: 5.0,
                label_idx: 2,
                success: false,
            },
        ];

        let filtered =
            filter_segments_with_indices_within_range(&segments, 2_100_000_000, 5_000_000_000);

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].0, 1);
        assert_eq!(filtered[0].1.label_idx, 1);
        assert_eq!(filtered[1].0, 2);
        assert_eq!(filtered[1].1.label_idx, 2);
    }
}

fn normalize_structs_to_lists(
    mut df: DataFrame,
) -> Result<DataFrame, Box<dyn Error + Send + Sync>> {
    let struct_column_names: Vec<String> = df
        .schema()
        .iter()
        .filter(|(_, dtype)| matches!(dtype, DataType::Struct(_)))
        .map(|(name, _)| name.to_string())
        .collect();

    for column_name in struct_column_names {
        let series = df.column(&column_name)?.as_materialized_series().clone();
        let flattened = struct_series_to_list(&series)?;
        df.replace(&column_name, flattened)?;
    }

    Ok(df)
}

fn struct_series_to_list(series: &Series) -> Result<Series, Box<dyn Error + Send + Sync>> {
    let primitive_type = find_first_primitive_type(series)?;

    match primitive_type {
        DataType::Float64 => struct_to_list_f64(series),
        DataType::Float32 => struct_to_list_f32(series),
        DataType::Int64 => struct_to_list_i64(series),
        DataType::Int32 => struct_to_list_i32(series),
        DataType::Int16 => struct_to_list_i16(series),
        DataType::Int8 => struct_to_list_i8(series),
        DataType::UInt64 => struct_to_list_u64(series),
        DataType::UInt32 => struct_to_list_u32(series),
        DataType::UInt16 => struct_to_list_u16(series),
        DataType::UInt8 => struct_to_list_u8(series),
        _ => Err(format!("Unsupported primitive type: {:?}", primitive_type).into()),
    }
}

fn find_first_primitive_type(series: &Series) -> Result<DataType, Box<dyn Error + Send + Sync>> {
    match series.dtype() {
        DataType::Struct(_) => {
            let struct_array = series.struct_()?;
            let fields = struct_array.fields_as_series();
            if fields.is_empty() {
                return Err("Empty struct has no primitive type".into());
            }
            find_first_primitive_type(&fields[0])
        }
        DataType::List(inner) => find_first_primitive_type_from_datatype(inner),
        dt if is_primitive_type(dt) => Ok(dt.clone()),
        dt => Err(format!("Unexpected type: {:?}", dt).into()),
    }
}

fn find_first_primitive_type_from_datatype(
    dtype: &DataType,
) -> Result<DataType, Box<dyn Error + Send + Sync>> {
    match dtype {
        DataType::Struct(fields) => {
            if fields.is_empty() {
                return Err("Empty struct has no primitive type".into());
            }
            find_first_primitive_type_from_datatype(fields[0].dtype())
        }
        DataType::List(inner) => find_first_primitive_type_from_datatype(inner),
        dt if is_primitive_type(dt) => Ok(dt.clone()),
        dt => Err(format!("Unexpected type: {:?}", dt).into()),
    }
}

fn is_primitive_type(dtype: &DataType) -> bool {
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

macro_rules! impl_struct_to_list_typed {
    ($($type_name:ident => $method:ident => $chunk:ty),* $(,)?) => {
        $(
            fn $type_name(series: &Series) -> Result<Series, Box<dyn Error + Send + Sync>> {
                let struct_array = series.struct_()?;
                let row_count = series.len();

                let extract = |s: &Series, idx: usize| -> Option<_> {
                    s.$method().ok()?.get(idx)
                };

                let all_rows: Vec<Vec<_>> = (0..row_count)
                    .map(|row_idx| {
                        let mut row_values = Vec::new();
                        for field in struct_array.fields_as_series() {
                            extract_values_at_row_with(&field, row_idx, &mut row_values, extract);
                        }
                        row_values
                    })
                    .collect();

                let list_chunked = all_rows
                    .into_iter()
                    .map(|row| {
                        let ca: ChunkedArray<$chunk> = ChunkedArray::from_vec("item".into(), row);
                        ca.into_series()
                    })
                    .collect::<ListChunked>();

                Ok(list_chunked.into_series().with_name(series.name().clone()))
            }
        )*
    };
}

impl_struct_to_list_typed! {
    struct_to_list_f64 => f64 => Float64Type,
    struct_to_list_f32 => f32 => Float32Type,
    struct_to_list_i64 => i64 => Int64Type,
    struct_to_list_i32 => i32 => Int32Type,
    struct_to_list_i16 => i16 => Int16Type,
    struct_to_list_i8 => i8 => Int8Type,
    struct_to_list_u64 => u64 => UInt64Type,
    struct_to_list_u32 => u32 => UInt32Type,
    struct_to_list_u16 => u16 => UInt16Type,
    struct_to_list_u8 => u8 => UInt8Type,
}

fn extract_values_at_row_with<T>(
    series: &Series,
    row_idx: usize,
    output: &mut Vec<T>,
    extract_primitive: impl Fn(&Series, usize) -> Option<T> + Copy,
) {
    match series.dtype() {
        DataType::Struct(_) => {
            // SAFETY: dtype is verified as Struct in match arm
            #[allow(clippy::expect_used)]
            let struct_series = series.struct_().expect("dtype verified as Struct");
            for field in struct_series.fields_as_series() {
                extract_values_at_row_with(&field, row_idx, output, extract_primitive);
            }
        }
        DataType::List(inner) => {
            // SAFETY: dtype is verified as List in match arm
            #[allow(clippy::expect_used)]
            let list_series = series.list().expect("dtype verified as List");
            if let Some(list_anyvalue) = list_series.get_as_series(row_idx) {
                if matches!(inner.as_ref(), DataType::Struct(_)) {
                    // SAFETY: inner type is verified as Struct
                    #[allow(clippy::expect_used)]
                    let struct_series = list_anyvalue
                        .struct_()
                        .expect("inner dtype verified as Struct");
                    for field in struct_series.fields_as_series() {
                        extract_values_from_series_with(&field, output, extract_primitive);
                    }
                } else {
                    extract_values_from_series_with(&list_anyvalue, output, extract_primitive);
                }
            }
        }
        _ => {
            if let Some(value) = extract_primitive(series, row_idx) {
                output.push(value);
            }
        }
    }
}

fn extract_values_from_series_with<T>(
    series: &Series,
    output: &mut Vec<T>,
    extract_primitive: impl Fn(&Series, usize) -> Option<T> + Copy,
) {
    match series.dtype() {
        DataType::Struct(_) => {
            // SAFETY: dtype is verified as Struct in match arm
            #[allow(clippy::expect_used)]
            let struct_series = series.struct_().expect("dtype verified as Struct");
            for field in struct_series.fields_as_series() {
                extract_values_from_series_with(&field, output, extract_primitive);
            }
        }
        DataType::List(_) => {
            // SAFETY: dtype is verified as List in match arm
            #[allow(clippy::expect_used)]
            let list_series = series.list().expect("dtype verified as List");
            for inner_series in list_series.into_iter().flatten() {
                for idx in 0..inner_series.len() {
                    if let Some(value) = extract_primitive(&inner_series, idx) {
                        output.push(value);
                    }
                }
            }
        }
        _ => {
            for idx in 0..series.len() {
                if let Some(value) = extract_primitive(series, idx) {
                    output.push(value);
                }
            }
        }
    }
}

/// Filter segments that overlap with the given timeline range.
///
/// A segment is included if its time range intersects with `[timeline_start, timeline_end]`.
pub(crate) fn filter_segments_with_indices_within_range(
    segments: &[Segment],
    timeline_start: u64,
    timeline_end: u64,
) -> Vec<(usize, Segment)> {
    segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| segment_overlaps_timeline(segment, timeline_start, timeline_end))
        .map(|(index, segment)| (index, segment.clone()))
        .collect()
}

fn segment_overlaps_timeline(segment: &Segment, timeline_start: u64, timeline_end: u64) -> bool {
    let segment_start = (segment.start_time * 1_000_000_000.0).round() as u64;
    let segment_end = (segment.end_time * 1_000_000_000.0).round() as u64;
    segment_end >= timeline_start && segment_start <= timeline_end
}

/// Concatenate multiple segment DataFrames into a single DataFrame.
///
/// # Errors
/// Returns an error if `frames` is empty.
pub(crate) fn concatenate_segment_frames(frames: Vec<DataFrame>) -> Result<DataFrame, StageError> {
    let mut iter = frames.into_iter();
    let mut combined = iter
        .next()
        .ok_or_else(|| StageError::invalid("no segment frames were produced"))?;

    for frame in iter {
        combined.vstack_mut(&frame)?;
    }

    Ok(combined)
}
