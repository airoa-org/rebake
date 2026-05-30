//! LeRobot v2.1 dataset merging.
//!
//! Merges multiple LeRobot v2.1 datasets into a single unified dataset.
//! Handles episode renumbering, task deduplication, parquet column remapping,
//! video file copying, and metadata consolidation.
//!
//! # Responsibilities
//!
//! | File | Role |
//! |------|------|
//! | `merger.rs` | Types, orchestration, source loading, task dedup, video copy, metadata merge |
//! | `parquet.rs` | Parquet column remapping via Polars (episode_index, index, task indices) |
//! | `validation.rs` | Source dataset compatibility checks (FPS, features, version) |

mod merger;
mod parquet;
mod validation;

pub use merger::{MergeConfig, discover_datasets, merge_datasets};
