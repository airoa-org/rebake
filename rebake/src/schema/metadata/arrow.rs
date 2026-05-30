use arrow::datatypes::FieldRef;
use arrow::record_batch::RecordBatch;
use serde_arrow::schema::{SchemaLike, TracingOptions};

use super::AiroaMetadata;
use super::v1_3::MetadataV1_3;
use super::v2_0::MetadataV2_0;
use crate::core::StageError;

/// Convert MetadataV2_0 to an Arrow RecordBatch.
///
/// This preserves the full nested structure of the metadata:
/// - `uuid` becomes `LargeUtf8` (UUID stored as string)
/// - `robot` becomes `Struct<uri, robot_type, id, checksum>`
/// - `files` becomes `List<Struct<file_type, name, checksum>>`
/// - `environment` becomes `Struct<env_type, site, location>`
/// - `runner` becomes `Struct<runner_type, organization, name>`
/// - `devices` becomes `List<Struct<role, device_type, id>>`
/// - `programs` becomes `List<Struct<role, name, source>>`
/// - `episode` becomes `Struct<start_time, end_time, success, label>`
/// - `labels` becomes `List<String>`
/// - `segments` becomes `List<Struct<start_time, end_time, label_idx, success>>`
///
/// Uses `from_type` for compile-time schema inference, ensuring `Option<T>`
/// fields always get the correct nullable type even when all values are `None`.
pub fn metadata_to_record_batch(metadata: &MetadataV2_0) -> Result<RecordBatch, StageError> {
    let tracing_options = TracingOptions::default().enums_without_data_as_strings(true);
    let fields = Vec::<FieldRef>::from_type::<MetadataV2_0>(tracing_options)
        .map_err(|e| StageError::invalid(format!("failed to get schema: {e}")))?;

    serde_arrow::to_record_batch(&fields, &[metadata]).map_err(|e| {
        StageError::invalid(format!("failed to convert metadata to record batch: {e}"))
    })
}

/// Get the Arrow schema for MetadataV2_0.
///
/// Uses `from_type` for compile-time schema inference.
pub fn metadata_arrow_schema() -> Result<Vec<FieldRef>, StageError> {
    let tracing_options = TracingOptions::default().enums_without_data_as_strings(true);
    Vec::<FieldRef>::from_type::<MetadataV2_0>(tracing_options)
        .map_err(|e| StageError::invalid(format!("failed to get schema: {e}")))
}

/// Convert MetadataV1_3 to an Arrow RecordBatch.
///
/// Uses `from_type` for compile-time schema inference, ensuring `Option<T>`
/// fields always get the correct nullable type even when all values are `None`.
pub fn v1_3_metadata_to_record_batch(metadata: &MetadataV1_3) -> Result<RecordBatch, StageError> {
    let fields = Vec::<FieldRef>::from_type::<MetadataV1_3>(TracingOptions::default())
        .map_err(|e| StageError::invalid(format!("failed to get schema: {e}")))?;

    serde_arrow::to_record_batch(&fields, &[metadata]).map_err(|e| {
        StageError::invalid(format!(
            "failed to convert V1.3 metadata to record batch: {e}"
        ))
    })
}

/// Convert AiroaMetadata to an Arrow RecordBatch.
///
/// Dispatches to version-specific functions:
/// - V1.3: Uses `v1_3_metadata_to_record_batch`
/// - V2.0: Uses `metadata_to_record_batch`
///
/// Both versions use `from_type` for compile-time schema inference,
/// ensuring `Option<T>` fields always get the correct nullable type.
pub fn airoa_metadata_to_record_batch(metadata: &AiroaMetadata) -> Result<RecordBatch, StageError> {
    match metadata {
        AiroaMetadata::V1_3(v1_3) => v1_3_metadata_to_record_batch(v1_3),
        AiroaMetadata::V2_0(v2_0) => metadata_to_record_batch(v2_0),
    }
}

/// Convert an Arrow RecordBatch containing canonical V2.0 metadata back into Rust.
pub fn record_batch_to_metadata(batch: &RecordBatch) -> Result<MetadataV2_0, StageError> {
    let items: Vec<MetadataV2_0> = serde_arrow::from_record_batch(batch).map_err(|e| {
        StageError::invalid(format!("failed to convert record batch to metadata: {e}"))
    })?;

    match items.as_slice() {
        [metadata] => Ok(metadata.clone()),
        [] => Err(StageError::invalid("metadata parquet is empty")),
        _ => Err(StageError::invalid(
            "metadata parquet must contain exactly one row",
        )),
    }
}
