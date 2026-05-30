//! Error types for the rebake pipeline.
//!
//! This module provides a unified error handling system for all pipeline stages.
//! The main error type is [`StageError`], which represents errors that can occur
//! during stage execution.
//!
//! # Quick Reference
//!
//! | Variant | Use When |
//! |---------|----------|
//! | [`MissingData`](StageError::MissingData) | Required data is not in Context or dataset |
//! | [`InvalidData`](StageError::InvalidData) | Data exists but format/value is wrong |
//! | [`Io`](StageError::Io) | File or network operation failed |
//! | [`Skip`](StageError::Skip) | Stage should be skipped (currently stops pipeline) |
//! | [`External`](StageError::External) | Error from external library |
//!
//! # Important Note on Skip
//!
//! Although `Skip` is conceptually intended for non-fatal cases,
//! the current `Orchestrator` implementation stops the pipeline on any error,
//! including `Skip`. See the usage guide for details.
//!
//! # Design Principles
//!
//! - **Few variants**: Keep the number of error variants small to reduce complexity
//! - **Preserve source**: Always keep the original error for debugging
//! - **Clear messages**: Use simple, readable English for error messages
//!
//! # Example
//!
//! ```
//! use rebake::core::error::{StageError, StageResult};
//!
//! fn process_data() -> StageResult<()> {
//!     // Check for missing data
//!     let data: Option<i32> = None;
//!     let _value = data.ok_or_else(|| StageError::missing("data in context"))?;
//!     Ok(())
//! }
//!
//! // The function returns an error because data is None
//! assert!(process_data().is_err());
//! ```

use std::io;

use thiserror::Error;

/// A boxed error type for dynamic error handling.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A Result type alias for stage operations.
pub type StageResult<T> = Result<T, StageError>;

/// Errors that can occur during stage execution.
///
/// This enum represents all possible errors from pipeline stages.
/// It is designed to be simple and easy to understand.
///
/// # Variants
///
/// - `MissingData`: Required data is not found in context or dataset
/// - `InvalidData`: Input data has wrong format or values
/// - `Io`: File or network operation failed
/// - `Skip`: Stage should be skipped (currently stops pipeline)
/// - `External`: Error from external library (polars, ffmpeg, etc.)
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StageError {
    /// Required data is not found in context.
    ///
    /// Use this when a stage needs data that should have been
    /// provided by a previous stage or configuration.
    ///
    /// # When to Use
    ///
    /// - Context field is `None` when the stage expects it
    /// - Expected topic is not present in the dataset
    /// - Required configuration parameter is missing
    ///
    /// # See Also
    ///
    /// - [`OptionExt::or_missing`] for convenient conversion
    /// - [`StageError::InvalidData`] for data that exists but is malformed
    #[error("missing required data: {0}")]
    MissingData(String),

    /// Input data has wrong format or invalid values.
    ///
    /// Use this when data exists but cannot be processed
    /// due to format issues or validation failures.
    ///
    /// # When to Use
    ///
    /// - DataFrame column is missing or has wrong type
    /// - Parsing failed (JSON, YAML, etc.)
    /// - Value is out of expected range (e.g., negative timestamp)
    /// - Schema mismatch
    ///
    /// # See Also
    ///
    /// - [`PolarsExt::or_invalid`] for Polars operations
    /// - [`StageError::invalid_with`] to preserve source error
    /// - [`StageError::MissingData`] for data that doesn't exist at all
    #[error("invalid data: {message}")]
    InvalidData {
        message: String,
        #[source]
        source: Option<BoxError>,
    },

    /// File or network operation failed.
    ///
    /// Use this for any I/O related errors like file not found,
    /// permission denied, or disk full.
    ///
    /// # When to Use
    ///
    /// - File open/read/write failed
    /// - Directory creation failed
    /// - Permission denied or disk full
    ///
    /// # See Also
    ///
    /// - [`StageError::io`] constructor
    /// - [`StageError::External`] for errors from external libraries (not `std::io`)
    #[error("I/O error: {context}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },

    /// Stage should be skipped.
    ///
    /// **Important**: Although conceptually intended for non-fatal cases,
    /// the current `Orchestrator` implementation stops the pipeline on any error,
    /// including `Skip`. Future versions may allow pipeline continuation.
    ///
    /// Use this when:
    /// - The stage determines it should not process the current data
    /// - Optional prerequisites are not met
    #[error("skipped: {reason}")]
    Skip { reason: String },

    /// Error from external library.
    ///
    /// Use this for errors from polars, ffmpeg, or other
    /// third-party libraries.
    ///
    /// # When to Use
    ///
    /// - Polars DataFrame operations failed
    /// - FFmpeg encoding/decoding failed
    /// - MCAP parsing failed
    /// - Other third-party crate errors
    ///
    /// # See Also
    ///
    /// - [`ResultExt::with_context`] for convenient wrapping
    /// - [`StageError::external_boxed`] for pre-boxed errors
    /// - [`StageError::Io`] for `std::io::Error` (use that instead)
    #[error("{context}")]
    External {
        context: String,
        #[source]
        source: BoxError,
    },
}

impl StageError {
    /// Creates an error for missing required data.
    ///
    /// # Example
    ///
    /// ```
    /// use rebake::core::error::StageError;
    ///
    /// let err = StageError::missing("dataset in context");
    /// assert!(err.is_missing());
    /// ```
    pub fn missing(what: impl Into<String>) -> Self {
        Self::MissingData(what.into())
    }

    /// Creates an error for invalid data without a source error.
    ///
    /// # Example
    ///
    /// ```
    /// use rebake::core::error::StageError;
    ///
    /// let err = StageError::invalid("value must be positive");
    /// assert_eq!(err.to_string(), "invalid data: value must be positive");
    /// ```
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidData {
            message: message.into(),
            source: None,
        }
    }

    /// Creates an error for invalid data with a source error.
    ///
    /// Use this when the invalid data was detected by parsing
    /// or validation that returned an error.
    ///
    /// # Example
    ///
    /// ```
    /// use rebake::core::error::StageError;
    ///
    /// let text = "not_a_number";
    /// let result: Result<i32, _> = text.parse();
    /// let err = result.map_err(|e| StageError::invalid_with("failed to parse number", e));
    /// assert!(err.is_err());
    /// ```
    pub fn invalid_with<E>(message: impl Into<String>, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::InvalidData {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Creates an I/O error with context.
    ///
    /// # Example
    ///
    /// ```
    /// use rebake::core::error::StageError;
    /// use std::io;
    ///
    /// let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    /// let err = StageError::io("failed to open config.yaml", io_err);
    /// assert!(err.is_io());
    /// ```
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    /// Creates a skip error.
    ///
    /// Use this when the stage determines it should not process the current data.
    /// Currently, the Orchestrator stops the pipeline on Skip.
    /// Future versions may allow pipeline continuation.
    ///
    /// # Example
    ///
    /// ```
    /// use rebake::core::error::StageError;
    ///
    /// let err = StageError::skip("no data to process");
    /// assert!(err.is_skip());
    /// ```
    pub fn skip(reason: impl Into<String>) -> Self {
        Self::Skip {
            reason: reason.into(),
        }
    }

    /// Creates an error for external library failures.
    ///
    /// Use this when an error comes from polars, ffmpeg,
    /// or other third-party libraries.
    ///
    /// # Example
    ///
    /// ```
    /// use rebake::core::error::StageError;
    /// use std::io;
    ///
    /// let external_err = io::Error::new(io::ErrorKind::Other, "external failure");
    /// let err = StageError::external("failed to process data", external_err);
    /// assert!(err.to_string().contains("failed to process data"));
    /// ```
    pub fn external<E>(context: impl Into<String>, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::External {
            context: context.into(),
            source: Box::new(source),
        }
    }

    /// Creates an error for external library failures from a boxed error.
    ///
    /// Use this when the error is already boxed, typically from functions
    /// that return `Box<dyn Error + Send + Sync>`.
    ///
    /// # Example
    ///
    /// ```
    /// use rebake::core::error::{StageError, BoxError};
    /// use std::io;
    ///
    /// let boxed_err: BoxError = Box::new(io::Error::new(io::ErrorKind::Other, "boxed error"));
    /// let err = StageError::external_boxed("operation failed", boxed_err);
    /// assert!(err.to_string().contains("operation failed"));
    /// ```
    pub fn external_boxed(context: impl Into<String>, source: BoxError) -> Self {
        Self::External {
            context: context.into(),
            source,
        }
    }

    /// Returns true if this is a skip error.
    ///
    /// Note: Although conceptually Skip indicates the stage should be skipped,
    /// the current Orchestrator treats it as fatal and stops the pipeline.
    pub fn is_skip(&self) -> bool {
        matches!(self, Self::Skip { .. })
    }

    /// Returns true if this is a missing data error.
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::MissingData(_))
    }

    /// Returns true if this is an I/O error.
    pub fn is_io(&self) -> bool {
        matches!(self, Self::Io { .. })
    }

    /// Returns the full error message including the source chain.
    ///
    /// Walks the [`std::error::Error::source()`] chain and joins all messages
    /// with `": "`, ensuring that nested error context (e.g., from
    /// [`External`](Self::External)) is not lost when converting errors to
    /// strings at the PyO3 boundary.
    ///
    /// # Example
    ///
    /// ```
    /// use rebake::core::error::StageError;
    /// use std::io;
    ///
    /// let inner = io::Error::new(io::ErrorKind::NotFound, "file.mp4 not found");
    /// let err = StageError::external("failed to encode video", inner);
    /// assert_eq!(
    ///     err.reason(),
    ///     "failed to encode video: file.mp4 not found",
    /// );
    /// ```
    pub fn reason(&self) -> String {
        use std::fmt::Write;

        let mut msg = self.to_string();
        let mut current = std::error::Error::source(self);
        while let Some(cause) = current {
            let _ = write!(msg, ": {cause}");
            current = cause.source();
        }
        msg
    }
}

// Automatic conversion from io::Error
impl From<io::Error> for StageError {
    fn from(err: io::Error) -> Self {
        Self::Io {
            context: "I/O operation failed".into(),
            source: err,
        }
    }
}

// Automatic conversion from PolarsError
impl From<polars::error::PolarsError> for StageError {
    fn from(err: polars::error::PolarsError) -> Self {
        Self::External {
            context: "polars operation failed".into(),
            source: Box::new(err),
        }
    }
}

// Automatic conversion from serde_json::Error
impl From<serde_json::Error> for StageError {
    fn from(err: serde_json::Error) -> Self {
        Self::InvalidData {
            message: "failed to parse JSON".into(),
            source: Some(Box::new(err)),
        }
    }
}

// Automatic conversion from serde_yaml::Error
impl From<serde_yaml::Error> for StageError {
    fn from(err: serde_yaml::Error) -> Self {
        Self::InvalidData {
            message: "failed to parse YAML".into(),
            source: Some(Box::new(err)),
        }
    }
}

/// Extension trait for Option to convert to StageResult.
///
/// This trait provides convenient methods to convert `Option<T>`
/// to `StageResult<T>` with appropriate error messages.
pub trait OptionExt<T> {
    /// Converts Option to StageResult with a missing data error.
    ///
    /// # Example
    ///
    /// ```
    /// use rebake::core::error::OptionExt;
    ///
    /// let value: Option<i32> = Some(42);
    /// assert_eq!(value.or_missing("value").unwrap(), 42);
    ///
    /// let none: Option<i32> = None;
    /// assert!(none.or_missing("value").is_err());
    /// ```
    fn or_missing(self, what: &str) -> StageResult<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn or_missing(self, what: &str) -> StageResult<T> {
        self.ok_or_else(|| StageError::missing(what))
    }
}

/// Extension trait for Result to add context to errors.
///
/// This trait provides convenient methods to convert errors
/// from external libraries to StageError with context.
pub trait ResultExt<T, E> {
    /// Converts an error to StageError with context.
    ///
    /// # Example
    ///
    /// ```
    /// use rebake::core::error::ResultExt;
    /// use std::io;
    ///
    /// let result: Result<i32, io::Error> = Err(io::Error::new(io::ErrorKind::Other, "oops"));
    /// let stage_result = result.with_context("failed to process");
    /// assert!(stage_result.is_err());
    /// ```
    fn with_context(self, context: impl Into<String>) -> StageResult<T>;
}

impl<T, E> ResultExt<T, E> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn with_context(self, context: impl Into<String>) -> StageResult<T> {
        self.map_err(|e| StageError::external(context, e))
    }
}

/// Extension trait for Polars operations that may return errors.
///
/// This trait provides convenient methods to convert Polars operation
/// errors to `StageError::InvalidData` with descriptive messages.
/// Use this for Polars DataFrame/Series operations where failure
/// indicates invalid or unexpected data format.
///
/// # Example
///
/// ```
/// use rebake::core::error::PolarsExt;
/// use polars::prelude::*;
///
/// fn get_column_as_f64(df: &DataFrame) -> rebake::core::error::StageResult<&Float64Chunked> {
///     df.column("values")
///         .or_invalid("missing 'values' column")?
///         .f64()
///         .or_invalid("'values' column is not f64")
/// }
/// ```
pub trait PolarsExt<T> {
    /// Converts a Polars error to `StageError::InvalidData`.
    ///
    /// Use this when a Polars operation fails due to data format issues,
    /// such as accessing a non-existent column, type mismatches, or
    /// invalid array operations.
    fn or_invalid(self, msg: &str) -> StageResult<T>;
}

impl<T, E: std::fmt::Debug> PolarsExt<T> for Result<T, E> {
    fn or_invalid(self, msg: &str) -> StageResult<T> {
        self.map_err(|e| StageError::invalid(format!("{}: {:?}", msg, e)))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_error() {
        let err = StageError::missing("dataset");
        assert!(err.is_missing());
        assert!(!err.is_skip());
        assert_eq!(err.to_string(), "missing required data: dataset");
    }

    #[test]
    fn test_invalid_error() {
        let err = StageError::invalid("value must be positive");
        assert!(!err.is_missing());
        assert_eq!(err.to_string(), "invalid data: value must be positive");
    }

    #[test]
    fn test_skip_error() {
        let err = StageError::skip("no data to process");
        assert!(err.is_skip());
        assert!(!err.is_missing());
        assert_eq!(err.to_string(), "skipped: no data to process");
    }

    #[test]
    fn test_io_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err = StageError::io("failed to open config.yaml", io_err);
        assert!(err.is_io());
        assert!(err.to_string().contains("failed to open config.yaml"));
    }

    #[test]
    fn test_option_ext() {
        let none: Option<i32> = None;
        let result = none.or_missing("value");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_missing());

        let some: Option<i32> = Some(42);
        let result = some.or_missing("value");
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_result_ext() {
        let err: Result<i32, io::Error> = Err(io::Error::new(io::ErrorKind::NotFound, "not found"));
        let result = err.with_context("failed to read file");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("failed to read file")
        );
    }

    #[test]
    fn test_from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let stage_err: StageError = io_err.into();
        assert!(stage_err.is_io());
    }

    #[test]
    fn test_reason_includes_source_chain() {
        // External variant: reason() should include the source error
        let inner = io::Error::new(io::ErrorKind::NotFound, "cam.mp4 not found");
        let err = StageError::external("failed to assemble segment", inner);
        let reason = err.reason();
        assert!(
            reason.contains("failed to assemble segment"),
            "reason should contain context: {reason}"
        );
        assert!(
            reason.contains("cam.mp4 not found"),
            "reason should contain source: {reason}"
        );

        // Io variant: reason() should include the source io::Error
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let err = StageError::io("failed to write output", io_err);
        let reason = err.reason();
        assert!(reason.contains("failed to write output"));
        assert!(reason.contains("access denied"));

        // Variants without source: reason() == to_string()
        let err = StageError::missing("dataset");
        assert_eq!(err.reason(), err.to_string());
    }

    #[test]
    fn test_polars_ext() {
        // Test with a simple error type
        let err: Result<i32, &str> = Err("type mismatch");
        let result = err.or_invalid("column is not i32");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("column is not i32"));
        assert!(err_msg.contains("type mismatch"));

        // Test with success case
        let ok: Result<i32, &str> = Ok(42);
        let result = ok.or_invalid("column is not i32");
        assert_eq!(result.unwrap(), 42);
    }
}
