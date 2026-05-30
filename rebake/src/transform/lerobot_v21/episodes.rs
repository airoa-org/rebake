use std::fs::{self, File};

use camino::Utf8Path;
use serde::{Deserialize, Serialize};

use crate::core::stage::StageError;
use crate::schema::metadata::v2_0::MetadataV2_0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Episodes {
    pub episode_index: usize,
    /// Stable episode identifier.
    ///
    /// - SHT mode: `{uuid}`
    /// - PA mode: `{uuid}:{source_segment_index}`
    pub episode_id: String,
    pub tasks: Vec<String>,
    pub length: usize,
    pub bag_path: String,
    pub version: String,
    pub location_name: String,
    pub interface: String,
    pub git_hash: String,
    pub git_branch: String,
    pub interface_git_hash: String,
    pub interface_git_branch: String,
    pub pipeline_git_hash: String,
    pub pipeline_git_branch: String,
    pub label: String,
    pub hsr_id: String,
    pub task_type: String,
    pub task_success: bool,
    pub short_horizon_task: Vec<String>,
    pub primitive_action: Vec<String>,
    pub success_short_horizon_task: bool,
    pub uuid: String,
    /// Full metadata from airoa meta.json (V2.0 format).
    /// Contains robot, environment, runner, programs, episode, segments, labels, etc.
    pub metadata: MetadataV2_0,
}

pub(crate) fn format_episode_id(uuid: &str, source_segment_index: Option<usize>) -> String {
    match source_segment_index {
        Some(index) => format!("{uuid}:{index}"),
        None => uuid.to_string(),
    }
}

impl Episodes {
    /// Save the episode to episodes.jsonl, overwriting any existing content.
    /// Used for SHT mode where there's only one episode.
    pub fn save(&self, outdir: &Utf8Path) -> Result<(), StageError> {
        let path = outdir.join("meta/episodes.jsonl");
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(path)?;
        serde_json::to_writer(&mut file, self)?;
        Ok(())
    }

    /// Save multiple episodes to episodes.jsonl, overwriting any existing content.
    /// Used for PA mode where all episodes are collected and written at once.
    pub fn save_all(episodes: &[Episodes], outdir: &Utf8Path) -> Result<(), StageError> {
        use std::io::Write;

        let path = outdir.join("meta/episodes.jsonl");
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }

        let mut file = File::create(path)?;
        for episode in episodes {
            serde_json::to_writer(&mut file, episode)?;
            writeln!(file)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::format_episode_id;

    #[test]
    fn format_episode_id_uses_uuid_for_sht_episode() {
        assert_eq!(format_episode_id("test-uuid", None), "test-uuid");
    }

    #[test]
    fn format_episode_id_appends_source_segment_index_for_pa_episode() {
        assert_eq!(format_episode_id("test-uuid", Some(7)), "test-uuid:7");
    }
}
