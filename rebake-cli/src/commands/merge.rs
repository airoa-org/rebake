//! Merge subcommand for combining multiple LeRobot v2.1 datasets.
//!
//! Discovers all dataset subdirectories (containing meta/info.json) within
//! the given source directory, then merges them with renumbered indices,
//! task deduplication, and consolidated metadata.
//!
//! Typical workflow: `rebake run` -> `rebake merge`

use anyhow::Result;
use camino::Utf8PathBuf;

#[derive(clap::Args, Debug)]
pub struct MergeArgs {
    /// Path to a directory containing multiple LeRobot dataset subdirectories.
    #[arg(value_name = "SOURCE_DIR", value_hint = clap::ValueHint::DirPath)]
    pub source_dir: Utf8PathBuf,

    /// Path to the output merged dataset directory.
    #[arg(
        short,
        long,
        value_name = "DIR",
        value_hint = clap::ValueHint::DirPath
    )]
    pub output: Utf8PathBuf,

    /// Override chunk size (default: use value from first source).
    #[arg(long = "chunk-size", alias = "chunks-size", value_name = "N")]
    pub chunk_size: Option<usize>,
}

pub fn run_merge(args: MergeArgs) -> Result<()> {
    let config = rebake::merge::MergeConfig {
        source_dir: args.source_dir,
        output: args.output,
        chunks_size: args.chunk_size,
    };
    rebake::merge::merge_datasets(&config)?;
    println!("Merge completed successfully.");
    Ok(())
}
