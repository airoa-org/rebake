use std::collections::HashMap;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyDict, PyDictMethods, PyList, PyModuleMethods};

use rebake::analysis as rebake_analysis;
use rebake::analysis::{AnalysisError, SegmentMetricsRow, SegmentRelativeMetricsRow};
use rebake::schema::metadata::parse_metadata_as_v2_0;

#[pyo3::pymodule(name = "analysis")]
pub fn analysis(
    _py: pyo3::Python<'_>,
    m: &pyo3::Bound<'_, pyo3::types::PyModule>,
) -> pyo3::PyResult<()> {
    m.add_function(wrap_pyfunction!(py_slice_timestamps_to_window, m)?)?;
    m.add_function(wrap_pyfunction!(py_compute_message_intervals_ms, m)?)?;
    m.add_function(wrap_pyfunction!(py_compute_coverage_ratio, m)?)?;
    m.add_function(wrap_pyfunction!(py_compute_observed_hz, m)?)?;
    m.add_function(wrap_pyfunction!(py_compute_interval_stats, m)?)?;
    m.add_function(wrap_pyfunction!(py_compute_topic_timing_metrics, m)?)?;
    m.add_function(wrap_pyfunction!(py_compute_segment_metrics, m)?)?;
    m.add_function(wrap_pyfunction!(py_compute_segment_relative_metrics, m)?)?;
    Ok(())
}

#[pyfunction(name = "slice_timestamps_to_window", signature = (timestamps_ns, start_ns, end_ns))]
fn py_slice_timestamps_to_window(
    timestamps_ns: Bound<'_, PyAny>,
    start_ns: i64,
    end_ns: i64,
) -> PyResult<Vec<i64>> {
    let timestamps = extract_i64_sequence(&timestamps_ns, "timestamps_ns")?;
    rebake_analysis::slice_timestamps_to_window(&timestamps, start_ns, end_ns)
        .map_err(to_py_value_error)
}

#[pyfunction(name = "compute_message_intervals_ms", signature = (timestamps_ns,))]
fn py_compute_message_intervals_ms(timestamps_ns: Bound<'_, PyAny>) -> PyResult<Vec<f64>> {
    let timestamps = extract_i64_sequence(&timestamps_ns, "timestamps_ns")?;
    Ok(rebake_analysis::compute_message_intervals_ms(&timestamps))
}

#[pyfunction(name = "compute_coverage_ratio", signature = (timestamps_ns, start_ns, end_ns))]
fn py_compute_coverage_ratio(
    timestamps_ns: Bound<'_, PyAny>,
    start_ns: i64,
    end_ns: i64,
) -> PyResult<f64> {
    let timestamps = extract_i64_sequence(&timestamps_ns, "timestamps_ns")?;
    rebake_analysis::compute_coverage_ratio(&timestamps, start_ns, end_ns)
        .map_err(to_py_value_error)
}

#[pyfunction(name = "compute_observed_hz", signature = (timestamps_ns,))]
fn py_compute_observed_hz(timestamps_ns: Bound<'_, PyAny>) -> PyResult<Option<f64>> {
    let timestamps = extract_i64_sequence(&timestamps_ns, "timestamps_ns")?;
    Ok(rebake_analysis::compute_observed_hz(&timestamps))
}

#[pyfunction(name = "compute_interval_stats", signature = (intervals_ms,))]
fn py_compute_interval_stats(
    py: Python<'_>,
    intervals_ms: Bound<'_, PyAny>,
) -> PyResult<Py<PyDict>> {
    let intervals = extract_f64_sequence(&intervals_ms, "intervals_ms")?;
    let stats = rebake_analysis::compute_interval_stats(&intervals);

    let dict = PyDict::new(py);
    dict.set_item("median_interval_ms", stats.median_interval_ms)?;
    dict.set_item("interval_cv", stats.interval_cv)?;
    dict.set_item("max_interval_ms", stats.max_interval_ms)?;
    dict.set_item("max_interval_over_median", stats.max_interval_over_median)?;
    Ok(dict.unbind())
}

#[pyfunction(name = "compute_topic_timing_metrics", signature = (timestamps_ns, episode_start_ns, episode_end_ns))]
fn py_compute_topic_timing_metrics(
    py: Python<'_>,
    timestamps_ns: Bound<'_, PyAny>,
    episode_start_ns: i64,
    episode_end_ns: i64,
) -> PyResult<Py<PyDict>> {
    let timestamps = extract_i64_sequence(&timestamps_ns, "timestamps_ns")?;
    let metrics = rebake_analysis::compute_topic_timing_metrics(
        &timestamps,
        episode_start_ns,
        episode_end_ns,
    )
    .map_err(to_py_value_error)?;

    let dict = PyDict::new(py);
    dict.set_item("message_count", metrics.message_count)?;
    dict.set_item(
        "first_message_timestamp_ns",
        metrics.first_message_timestamp_ns,
    )?;
    dict.set_item(
        "last_message_timestamp_ns",
        metrics.last_message_timestamp_ns,
    )?;
    dict.set_item("observed_topic_span_s", metrics.observed_topic_span_s)?;
    dict.set_item("episode_coverage_ratio", metrics.episode_coverage_ratio)?;
    dict.set_item("observed_hz", metrics.observed_hz)?;
    dict.set_item("median_topic_interval_ms", metrics.median_topic_interval_ms)?;
    dict.set_item("topic_interval_cv", metrics.topic_interval_cv)?;
    dict.set_item("max_topic_interval_ms", metrics.max_topic_interval_ms)?;
    dict.set_item("max_interval_over_median", metrics.max_interval_over_median)?;
    Ok(dict.unbind())
}

#[pyfunction(
    name = "compute_segment_metrics",
    signature = (topic_timestamps_ns, metadata_json, required_topics)
)]
fn py_compute_segment_metrics(
    py: Python<'_>,
    topic_timestamps_ns: Bound<'_, PyAny>,
    metadata_json: &str,
    required_topics: Vec<String>,
) -> PyResult<Py<PyList>> {
    let topic_timestamps = extract_i64_mapping(&topic_timestamps_ns, "topic_timestamps_ns")?;
    let metadata = parse_metadata_as_v2_0(metadata_json).map_err(|error| {
        PyValueError::new_err(format!(
            "metadata_json must contain valid Airoa metadata: {error}"
        ))
    })?;
    let rows =
        rebake_analysis::compute_segment_metrics(&metadata, &topic_timestamps, &required_topics)
            .map_err(to_py_value_error)?;

    let list = PyList::empty(py);
    for row in rows {
        list.append(segment_metrics_row_to_pydict(py, &row)?)?;
    }
    Ok(list.unbind())
}

#[pyfunction(name = "compute_segment_relative_metrics", signature = (segment_metrics_json,))]
fn py_compute_segment_relative_metrics(
    py: Python<'_>,
    segment_metrics_json: &str,
) -> PyResult<Py<PyList>> {
    let segment_metrics = serde_json::from_str::<Vec<SegmentMetricsRow>>(segment_metrics_json)
        .map_err(|error| {
            PyValueError::new_err(format!(
                "segment_metrics_json must be a JSON array of segment metrics rows: {error}"
            ))
        })?;
    let rows = rebake_analysis::compute_segment_relative_metrics(&segment_metrics);

    let list = PyList::empty(py);
    for row in rows {
        list.append(segment_relative_metrics_row_to_pydict(py, &row)?)?;
    }
    Ok(list.unbind())
}

fn extract_i64_sequence(values: &Bound<'_, PyAny>, name: &str) -> PyResult<Vec<i64>> {
    values.extract::<Vec<i64>>().map_err(|_| {
        PyValueError::new_err(format!(
            "{name} must be a one-dimensional sequence of integers"
        ))
    })
}

fn extract_f64_sequence(values: &Bound<'_, PyAny>, name: &str) -> PyResult<Vec<f64>> {
    values.extract::<Vec<f64>>().map_err(|_| {
        PyValueError::new_err(format!(
            "{name} must be a one-dimensional sequence of numbers"
        ))
    })
}

fn extract_i64_mapping(
    values: &Bound<'_, PyAny>,
    name: &str,
) -> PyResult<HashMap<String, Vec<i64>>> {
    let mapping = values.cast::<PyDict>().map_err(|_| {
        PyValueError::new_err(format!(
            "{name} must be a mapping from topic name to one-dimensional integer sequence"
        ))
    })?;

    let mut result = HashMap::with_capacity(mapping.len());
    for (key, value) in mapping.iter() {
        let topic_name = key
            .extract::<String>()
            .map_err(|_| PyValueError::new_err(format!("{name} keys must be strings")))?;
        let timestamps = extract_i64_sequence(&value, name)?;
        result.insert(topic_name, timestamps);
    }

    Ok(result)
}

fn segment_metrics_row_to_pydict(py: Python<'_>, row: &SegmentMetricsRow) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("recording_uuid", &row.recording_uuid)?;
    dict.set_item("segment_index", row.segment_index)?;
    dict.set_item("segment_label", &row.segment_label)?;
    dict.set_item("segment_success", row.segment_success)?;
    dict.set_item("segment_duration_s", row.segment_duration_s)?;
    dict.set_item("has_all_required_topics", row.has_all_required_topics)?;
    dict.set_item(
        "minimum_required_segment_coverage_ratio",
        row.minimum_required_segment_coverage_ratio,
    )?;
    dict.set_item(
        "worst_topic_max_interval_ms",
        row.worst_topic_max_interval_ms,
    )?;
    dict.set_item(
        "worst_topic_max_interval_over_median",
        row.worst_topic_max_interval_over_median,
    )?;
    Ok(dict.unbind())
}

fn segment_relative_metrics_row_to_pydict(
    py: Python<'_>,
    row: &SegmentRelativeMetricsRow,
) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("recording_uuid", &row.recording_uuid)?;
    dict.set_item("segment_index", row.segment_index)?;
    dict.set_item("segment_label", &row.segment_label)?;
    dict.set_item("segment_label_group_size", row.segment_label_group_size)?;
    dict.set_item(
        "segment_duration_s_percentile_rank_within_segment_label",
        row.segment_duration_s_percentile_rank_within_segment_label,
    )?;
    Ok(dict.unbind())
}

fn to_py_value_error(error: AnalysisError) -> PyErr {
    PyValueError::new_err(error.to_string())
}
