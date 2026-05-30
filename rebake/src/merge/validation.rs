use crate::core::error::{StageError, StageResult};
use crate::transform::lerobot_v21::DType;

use super::merger::SourceDataset;

/// Validate that all source datasets are compatible for merging.
///
/// Checks:
/// - At least one source dataset is provided
/// - All datasets have the same FPS
/// - All datasets have the same codebase_version
/// - All datasets have compatible feature schemas (same keys, dtypes, and shapes)
pub fn validate_sources(sources: &[SourceDataset]) -> StageResult<()> {
    if sources.len() < 2 {
        return Err(StageError::invalid(
            "at least two source datasets are required for merging",
        ));
    }

    let reference = &sources[0];

    for (i, source) in sources.iter().enumerate().skip(1) {
        // Validate FPS
        if source.info.fps != reference.info.fps {
            return Err(StageError::invalid(format!(
                "FPS mismatch: source 0 ({}) has fps={}, source {} ({}) has fps={}",
                reference.path, reference.info.fps, i, source.path, source.info.fps
            )));
        }

        // Validate codebase_version
        if source.info.codebase_version != reference.info.codebase_version {
            return Err(StageError::invalid(format!(
                "codebase_version mismatch: source 0 ({}) has '{}', source {} ({}) has '{}'",
                reference.path,
                reference.info.codebase_version,
                i,
                source.path,
                source.info.codebase_version
            )));
        }

        // Validate feature schemas
        validate_feature_compatibility(reference, source, i)?;
    }

    Ok(())
}

/// Validate that the feature schemas of two datasets are compatible.
///
/// Checks that both datasets have the same feature keys with matching dtypes and shapes.
/// For video features, also verifies that codec and pixel format match.
fn validate_feature_compatibility(
    reference: &SourceDataset,
    source: &SourceDataset,
    source_index: usize,
) -> StageResult<()> {
    let ref_features = &reference.info.features;
    let src_features = &source.info.features;

    // Check that both datasets have the same feature keys
    for (key, ref_feature) in ref_features {
        let src_feature = src_features.get(key).ok_or_else(|| {
            StageError::invalid(format!(
                "feature '{}' present in source 0 ({}) but missing in source {} ({})",
                key, reference.path, source_index, source.path
            ))
        })?;

        // Check dtype
        if ref_feature.dtype != src_feature.dtype {
            return Err(StageError::invalid(format!(
                "feature '{}' dtype mismatch: source 0 has {:?}, source {} has {:?}",
                key, ref_feature.dtype, source_index, src_feature.dtype
            )));
        }

        // Check shape
        if ref_feature.shape != src_feature.shape {
            return Err(StageError::invalid(format!(
                "feature '{}' shape mismatch: source 0 has {:?}, source {} has {:?}",
                key, ref_feature.shape, source_index, src_feature.shape
            )));
        }

        // For video features, also check codec and pixel format
        if ref_feature.dtype == DType::Video
            && let (Some(ref_info), Some(src_info)) =
                (&ref_feature.video_info, &src_feature.video_info)
        {
            if ref_info.codec != src_info.codec {
                return Err(StageError::invalid(format!(
                    "feature '{}' video codec mismatch: source 0 has '{}', source {} has '{}'",
                    key, ref_info.codec, source_index, src_info.codec
                )));
            }
            if ref_info.pix_fmt != src_info.pix_fmt {
                return Err(StageError::invalid(format!(
                    "feature '{}' video pix_fmt mismatch: source 0 has '{}', source {} has '{}'",
                    key, ref_info.pix_fmt, source_index, src_info.pix_fmt
                )));
            }
        }
    }

    // Check for extra features in source that are not in reference
    for key in src_features.keys() {
        if !ref_features.contains_key(key) {
            return Err(StageError::invalid(format!(
                "feature '{}' present in source {} ({}) but missing in source 0 ({})",
                key, source_index, source.path, reference.path
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use indexmap::IndexMap;

    use crate::transform::lerobot_v21::{DType, Feature, Info, VideoInfo};

    use super::*;

    fn make_source(path: &str, fps: usize, features: IndexMap<String, Feature>) -> SourceDataset {
        SourceDataset {
            path: path.into(),
            info: Info {
                codebase_version: "v2.1".to_string(),
                fps,
                features,
                ..Default::default()
            },
            tasks: vec![],
            episodes: vec![],
            episode_stats: vec![],
        }
    }

    #[test]
    fn test_validate_fps_mismatch() {
        let sources = vec![
            make_source("/a", 10, IndexMap::new()),
            make_source("/b", 20, IndexMap::new()),
        ];
        let err = validate_sources(&sources).unwrap_err();
        assert!(err.to_string().contains("FPS mismatch"));
    }

    #[test]
    fn test_validate_compatible_sources() {
        let features = IndexMap::from([(
            "observation.state".to_string(),
            Feature {
                dtype: DType::Float32,
                shape: vec![8],
                ..Default::default()
            },
        )]);
        let sources = vec![
            make_source("/a", 10, features.clone()),
            make_source("/b", 10, features),
        ];
        assert!(validate_sources(&sources).is_ok());
    }

    #[test]
    fn test_validate_feature_dtype_mismatch() {
        let features_a = IndexMap::from([(
            "observation.state".to_string(),
            Feature {
                dtype: DType::Float32,
                shape: vec![8],
                ..Default::default()
            },
        )]);
        let features_b = IndexMap::from([(
            "observation.state".to_string(),
            Feature {
                dtype: DType::Float64,
                shape: vec![8],
                ..Default::default()
            },
        )]);
        let sources = vec![
            make_source("/a", 10, features_a),
            make_source("/b", 10, features_b),
        ];
        let err = validate_sources(&sources).unwrap_err();
        assert!(err.to_string().contains("dtype mismatch"));
    }

    #[test]
    fn test_validate_feature_shape_mismatch() {
        let features_a = IndexMap::from([(
            "observation.state".to_string(),
            Feature {
                dtype: DType::Float32,
                shape: vec![8],
                ..Default::default()
            },
        )]);
        let features_b = IndexMap::from([(
            "observation.state".to_string(),
            Feature {
                dtype: DType::Float32,
                shape: vec![14],
                ..Default::default()
            },
        )]);
        let sources = vec![
            make_source("/a", 10, features_a),
            make_source("/b", 10, features_b),
        ];
        let err = validate_sources(&sources).unwrap_err();
        assert!(err.to_string().contains("shape mismatch"));
    }

    #[test]
    fn test_validate_missing_feature() {
        let features_a = IndexMap::from([(
            "observation.state".to_string(),
            Feature {
                dtype: DType::Float32,
                shape: vec![8],
                ..Default::default()
            },
        )]);
        let sources = vec![
            make_source("/a", 10, features_a),
            make_source("/b", 10, IndexMap::new()),
        ];
        let err = validate_sources(&sources).unwrap_err();
        assert!(err.to_string().contains("missing in source 1"));
    }

    #[test]
    fn test_validate_video_codec_mismatch() {
        let make_video_feature = |codec: &str| -> IndexMap<String, Feature> {
            IndexMap::from([(
                "observation.image.head".to_string(),
                Feature {
                    dtype: DType::Video,
                    shape: vec![480, 640, 3],
                    video_info: Some(VideoInfo {
                        fps: 10,
                        codec: codec.to_string(),
                        pix_fmt: "yuv420p".to_string(),
                        is_depth_map: false,
                        has_audio: false,
                    }),
                    ..Default::default()
                },
            )])
        };
        let sources = vec![
            make_source("/a", 10, make_video_feature("av1")),
            make_source("/b", 10, make_video_feature("h264")),
        ];
        let err = validate_sources(&sources).unwrap_err();
        assert!(err.to_string().contains("video codec mismatch"));
    }
}
