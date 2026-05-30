use std::sync::Arc;

use arrow::datatypes::{Field as ArrowField, Schema as ArrowSchema};
use arrow::record_batch::RecordBatch;
use arrow_array::ffi::{FFI_ArrowArray, from_ffi as arrow_from_ffi, to_ffi as arrow_to_ffi};
use arrow_array::make_array;
use arrow_schema::ffi::FFI_ArrowSchema;
use polars::prelude::*;
use polars_arrow::array::Array;
use polars_arrow::ffi::{
    ArrowArray as PolarsArrowArray, ArrowSchema as PolarsArrowSchema, export_array_to_c,
    export_field_to_c, import_array_from_c, import_field_from_c,
};
use polars_arrow::record_batch::RecordBatchT;

type PolarsBatch = RecordBatchT<Box<dyn Array>>;

pub fn polars_batch_to_arrow(batch: PolarsBatch) -> RecordBatch {
    let (schema, arrays) = batch.into_schema_and_arrays();
    let fields: Vec<_> = schema.iter_values().cloned().collect();

    let mut arrow_arrays = Vec::with_capacity(arrays.len());
    let mut arrow_fields = Vec::with_capacity(fields.len());

    for (field, array) in fields.iter().zip(arrays.into_iter()) {
        let polars_schema: PolarsArrowSchema = export_field_to_c(field);
        let polars_array: PolarsArrowArray = export_array_to_c(array);

        let arrow_schema_c: FFI_ArrowSchema = unsafe { std::mem::transmute(polars_schema) };
        let arrow_array_c: FFI_ArrowArray = unsafe { std::mem::transmute(polars_array) };

        // SAFETY: FFI conversion from Polars to Arrow arrays.
        // These operations should always succeed when the input arrays are valid,
        // as both libraries use compatible Arrow memory layouts. Failure here
        // indicates a bug in the FFI layer or incompatible library versions.
        #[allow(clippy::expect_used)]
        let arrow_data = unsafe { arrow_from_ffi(arrow_array_c, &arrow_schema_c) }
            .expect("FFI conversion to Arrow failed - incompatible array layout or library version mismatch");
        let arrow_array = make_array(arrow_data);
        // SAFETY: Arrow field conversion from FFI schema should always succeed
        // when the schema was correctly exported from Polars.
        #[allow(clippy::expect_used)]
        let arrow_field = ArrowField::try_from(&arrow_schema_c)
            .expect("Arrow field conversion failed - incompatible schema format");

        arrow_arrays.push(arrow_array);
        arrow_fields.push(arrow_field);
    }

    let arrow_schema = ArrowSchema::new(arrow_fields);
    // SAFETY: RecordBatch creation should always succeed when arrays and schema
    // are correctly converted from Polars. Failure indicates a mismatch between
    // the converted schema and arrays, which would be a bug in this function.
    #[allow(clippy::expect_used)]
    RecordBatch::try_new(Arc::new(arrow_schema), arrow_arrays)
        .expect("RecordBatch creation failed - schema/array mismatch in FFI conversion")
}

pub fn arrow_batch_to_polars(batch: &RecordBatch) -> DataFrame {
    let mut columns: Vec<Column> = Vec::with_capacity(batch.num_columns());

    for (field, array) in batch.schema().fields().iter().zip(batch.columns()) {
        let array_data = array.to_data();
        // SAFETY: Exporting Arrow arrays to FFI should always succeed for valid arrays.
        // Failure indicates a bug in the Arrow library or corrupted array data.
        #[allow(clippy::expect_used)]
        let (ffi_array, ffi_schema) = arrow_to_ffi(&array_data)
            .expect("Arrow to FFI export failed - corrupted array data or Arrow library bug");

        let polars_array: PolarsArrowArray = unsafe { std::mem::transmute(ffi_array) };
        let polars_schema: PolarsArrowSchema = unsafe { std::mem::transmute(ffi_schema) };

        // SAFETY: FFI conversion from Arrow to Polars field/array.
        // These operations should always succeed when the input is a valid Arrow array,
        // as both libraries use compatible Arrow memory layouts. Failure here
        // indicates a bug in the FFI layer or incompatible library versions.
        #[allow(clippy::expect_used)]
        let polars_field = unsafe { import_field_from_c(&polars_schema) }
            .expect("Polars field import failed - incompatible schema format");
        let dtype = polars_field.dtype().clone();
        // SAFETY: Array import should always succeed when dtype matches the exported array.
        #[allow(clippy::expect_used)]
        let polars_array = unsafe { import_array_from_c(polars_array, dtype.clone()) }
            .expect("Polars array import failed - incompatible array layout or type mismatch");

        let name = PlSmallStr::from_str(field.name());
        // SAFETY: Series creation from a successfully imported Polars array should always work.
        // The array and dtype are both derived from the same source, so they are guaranteed to match.
        #[allow(clippy::expect_used)]
        let series = unsafe { Series::_try_from_arrow_unchecked(name, vec![polars_array], &dtype) }
            .expect("Series creation failed - dtype mismatch in FFI conversion");

        columns.push(series.into());
    }

    // SAFETY: DataFrame creation from successfully converted columns should always work.
    // All columns have the same length (from the original RecordBatch) and unique names.
    #[allow(clippy::expect_used)]
    DataFrame::new(columns).expect("DataFrame creation failed - column length or name mismatch")
}

/// Converts a LazyFrame to a vector of Arrow RecordBatches.
///
/// # Panics
///
/// Panics if the LazyFrame cannot be collected. This can happen if:
/// - The query plan is invalid
/// - There are schema mismatches in the lazy operations
/// - Memory allocation fails
///
/// For fallible conversion, collect the LazyFrame first and handle the error.
pub fn lazy_to_record_batches_iter(lf: &LazyFrame) -> Vec<RecordBatch> {
    // SAFETY: LazyFrame operations are constructed by this crate and are valid.
    #[allow(clippy::expect_used)]
    let df = lf
        .clone()
        .collect()
        .expect("LazyFrame collection failed - invalid query plan or schema mismatch");
    df.iter_chunks(CompatLevel::oldest(), false)
        .map(polars_batch_to_arrow)
        .collect()
}

/// Converts a LazyFrame to a single rechunked Arrow RecordBatch.
///
/// # Panics
///
/// Panics if the LazyFrame cannot be collected. This can happen if:
/// - The query plan is invalid
/// - There are schema mismatches in the lazy operations
/// - Memory allocation fails
///
/// For fallible conversion, collect the LazyFrame first and handle the error.
pub fn lazy_to_record_batch_rechunk(lf: &LazyFrame) -> RecordBatch {
    // SAFETY: LazyFrame operations are constructed by this crate and are valid.
    #[allow(clippy::expect_used)]
    let df = lf
        .clone()
        .collect()
        .expect("LazyFrame collection failed - invalid query plan or schema mismatch");
    polars_batch_to_arrow(df.rechunk_to_record_batch(CompatLevel::oldest()))
}

pub fn record_batch_to_lazy(rb: &RecordBatch) -> LazyFrame {
    arrow_batch_to_polars(rb).lazy()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use arrow::datatypes::{DataType as ArrowDataType, Fields};

    fn sample_lazy_frame() -> LazyFrame {
        let mut df = df! {
            "name" => &["alice", "bob", "carol"],
            "age" => &[Some(30_i32), None, Some(45)],
            "score" => &[98.5_f64, 72.0, 88.25],
            "active" => &[true, false, true],
        }
        .unwrap();

        let mut history_builder =
            ListPrimitiveChunkedBuilder::<Int32Type>::new("history".into(), 3, 6, DataType::Int32);
        history_builder.append_slice(&[1, 2, 3]);
        history_builder.append_null();
        history_builder.append_slice(&[4, 5]);
        let history_series = history_builder.finish().into_series();

        let city_series = Series::new("city".into(), &["tokyo", "osaka", "nagoya"]);
        let floor_series = Series::new("floor".into(), &[Some(12_i32), Some(3), None]);

        let location_series = StructChunked::from_columns(
            PlSmallStr::from_str("location"),
            df.height(),
            &[city_series.into(), floor_series.into()],
        )
        .expect("struct chunked")
        .into_series();

        df.with_column(history_series).unwrap();
        df.with_column(location_series).unwrap();
        df.lazy()
    }

    #[test]
    fn roundtrip_iter_chunks_preserves_data() {
        let lf = sample_lazy_frame();
        let original = lf.clone().collect().unwrap();

        let batches = lazy_to_record_batches_iter(&lf);
        assert!(!batches.is_empty());

        let reconstructed = record_batch_to_lazy(&batches[0]);
        let reconstructed_df = reconstructed.collect().unwrap();

        assert!(original.equals_missing(&reconstructed_df));
    }

    #[test]
    fn roundtrip_rechunk_preserves_data() {
        let lf = sample_lazy_frame();
        let original = lf.clone().collect().unwrap();

        let batch = lazy_to_record_batch_rechunk(&lf);
        assert_record_batch_schema(&batch);
        let reconstructed = record_batch_to_lazy(&batch);
        let reconstructed_df = reconstructed.collect().unwrap();

        assert!(original.equals_missing(&reconstructed_df));
    }

    fn assert_record_batch_schema(batch: &RecordBatch) {
        let expected_fields = vec![
            ("name", ArrowDataType::Utf8),
            ("age", ArrowDataType::Int32),
            ("score", ArrowDataType::Float64),
            ("active", ArrowDataType::Boolean),
            (
                "history",
                ArrowDataType::List(Arc::new(ArrowField::new(
                    "item",
                    ArrowDataType::Int32,
                    true,
                ))),
            ),
            (
                "location",
                ArrowDataType::Struct(Fields::from(vec![
                    ArrowField::new("city", ArrowDataType::Utf8, true),
                    ArrowField::new("floor", ArrowDataType::Int32, true),
                ])),
            ),
        ];

        let batch_schema = batch.schema();
        assert_eq!(batch_schema.fields().len(), expected_fields.len());
        for (field, (name, dtype)) in batch_schema.fields().iter().zip(expected_fields) {
            assert_eq!(field.name(), name);
            assert!(
                dtype_matches(field.data_type(), &dtype),
                "expected dtype {dtype:?} but got {:?}",
                field.data_type()
            );
        }
    }

    fn dtype_matches(actual: &ArrowDataType, expected: &ArrowDataType) -> bool {
        use ArrowDataType::*;
        match (actual, expected) {
            (Utf8View, Utf8) | (Utf8, Utf8View) => true,
            (LargeUtf8, Utf8) | (Utf8, LargeUtf8) => true,
            (LargeUtf8, Utf8View) | (Utf8View, LargeUtf8) => true,
            (BinaryView, Binary) | (Binary, BinaryView) => true,
            (List(a), List(b)) | (List(a), LargeList(b)) | (LargeList(a), List(b)) => {
                dtype_matches(a.data_type(), b.data_type())
            }
            (LargeList(a), LargeList(b)) => dtype_matches(a.data_type(), b.data_type()),
            (Struct(a), Struct(b)) => {
                a.len() == b.len()
                    && a.iter().zip(b.iter()).all(|(fa, fb)| {
                        fa.name() == fb.name() && dtype_matches(fa.data_type(), fb.data_type())
                    })
            }
            _ => actual == expected,
        }
    }
}
