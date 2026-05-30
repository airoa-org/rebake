use polars::lazy::dsl::as_struct;
use polars::prelude::*;
use serde::{Deserialize, Serialize};

use crate::core::error::OptionExt;
use crate::core::stage::{Context, Stage, StageConfig, StageError};
use crate::enrich::expr::{QuaternionExpr, Vector3Expr};

const DELTA_FIELD: &str = "delta_transform";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaReferenceFrame {
    /// Translation is expressed as a source-frame coordinate component
    /// difference. Rotation preserves the existing relative quaternion delta.
    SourceFrame,
    /// Expresses the transform delta in the previous target frame.
    PreviousTargetFrame,
}

#[derive(Debug, Clone)]
struct TransformPath {
    components: Vec<String>,
}

impl TransformPath {
    fn new(components: Vec<String>) -> Self {
        Self { components }
    }

    fn root(&self) -> &str {
        &self.components[0]
    }

    fn len(&self) -> usize {
        self.components.len()
    }

    fn field_name(&self, index: usize) -> &str {
        &self.components[index]
    }

    fn build_expr(&self, delta_reference_frame: DeltaReferenceFrame) -> Expr {
        let delta_transform = build_delta_transform_expr(self, delta_reference_frame);

        if self.len() == 1 {
            return delta_transform;
        }

        let mut updated = struct_expr_at(self, self.len() - 2)
            .struct_()
            .with_fields(vec![delta_transform])
            .alias(self.field_name(self.len() - 2));

        for depth in (1..self.len() - 1).rev() {
            let parent_expr = struct_expr_at(self, depth - 1);
            updated = parent_expr
                .struct_()
                .with_fields(vec![updated])
                .alias(self.field_name(depth - 1));
        }

        updated
    }
}

struct TransformSchemaInspector<'a> {
    schema: &'a Schema,
}

impl<'a> TransformSchemaInspector<'a> {
    fn new(schema: &'a Schema) -> Self {
        Self { schema }
    }

    fn discover_paths(&self) -> Vec<TransformPath> {
        let mut paths = Vec::new();
        for field in self.schema.iter_fields() {
            Self::visit_field(vec![field.name().to_string()], &field, &mut paths);
        }
        paths
    }

    fn visit_field(current_path: Vec<String>, field: &Field, acc: &mut Vec<TransformPath>) {
        match field.dtype() {
            DataType::Struct(children) if field.name() == "transform" => {
                if Self::is_transform_struct(children.as_ref()) {
                    acc.push(TransformPath::new(current_path));
                }
            }
            DataType::Struct(children) => {
                for child in children.iter() {
                    let mut next_path = current_path.clone();
                    next_path.push(child.name().to_string());
                    Self::visit_field(next_path, child, acc);
                }
            }
            _ => {}
        }
    }

    fn is_transform_struct(fields: &[Field]) -> bool {
        let has_translation = fields
            .iter()
            .any(|field| field.name() == "translation" && Self::is_vector3_struct(field.dtype()));
        let has_rotation = fields
            .iter()
            .any(|field| field.name() == "rotation" && Self::is_quaternion_struct(field.dtype()));
        has_translation && has_rotation
    }

    fn is_vector3_struct(data_type: &DataType) -> bool {
        if let DataType::Struct(children) = data_type {
            ["x", "y", "z"].iter().all(|name| {
                children
                    .iter()
                    .find(|field| field.name().as_str() == *name)
                    .map(|field| matches!(field.dtype(), DataType::Float64 | DataType::Float32))
                    .unwrap_or(false)
            })
        } else {
            false
        }
    }

    fn is_quaternion_struct(data_type: &DataType) -> bool {
        if let DataType::Struct(children) = data_type {
            ["x", "y", "z", "w"].iter().all(|name| {
                children
                    .iter()
                    .find(|field| field.name().as_str() == *name)
                    .map(|field| matches!(field.dtype(), DataType::Float64 | DataType::Float32))
                    .unwrap_or(false)
            })
        } else {
            false
        }
    }
}

fn struct_expr_at(path: &TransformPath, depth: usize) -> Expr {
    let mut expr = col(path.root());
    for segment in path.components.iter().take(depth + 1).skip(1) {
        expr = expr.struct_().field_by_name(segment);
    }
    expr
}

fn component_expr(path: &TransformPath, extra_segments: &[&str]) -> Expr {
    let mut expr = struct_expr_at(path, path.len() - 1);
    for segment in extra_segments {
        expr = expr.struct_().field_by_name(segment);
    }
    expr
}

fn translation_components(path: &TransformPath) -> Vector3Expr {
    Vector3Expr::new(
        component_expr(path, &["translation", "x"]),
        component_expr(path, &["translation", "y"]),
        component_expr(path, &["translation", "z"]),
    )
}

fn quaternion_components(path: &TransformPath) -> QuaternionExpr {
    QuaternionExpr::new(
        component_expr(path, &["rotation", "w"]),
        component_expr(path, &["rotation", "x"]),
        component_expr(path, &["rotation", "y"]),
        component_expr(path, &["rotation", "z"]),
    )
}

fn translation_delta_expr(
    path: &TransformPath,
    delta_reference_frame: DeltaReferenceFrame,
) -> Expr {
    let current = translation_components(path);
    let previous = current.shifted(lit(1));
    let source_delta = current.delta(&previous);
    let delta = match delta_reference_frame {
        DeltaReferenceFrame::SourceFrame => source_delta,
        DeltaReferenceFrame::PreviousTargetFrame => {
            let previous_rotation = quaternion_components(path).shifted(lit(1));
            previous_rotation.inverse_rotate_vector(&source_delta)
        }
    };
    delta.fill_null(0.0).into_struct("translation")
}

fn rotation_delta_expr(path: &TransformPath) -> Expr {
    let current = quaternion_components(path);
    let previous = current.shifted(lit(1));
    let aligned = current.align_to_shortest_arc(&previous);
    aligned
        .delta(&previous)
        .fill_null(0.0, 1.0)
        .into_struct("rotation")
}

fn build_delta_transform_expr(
    path: &TransformPath,
    delta_reference_frame: DeltaReferenceFrame,
) -> Expr {
    let translation_delta = translation_delta_expr(path, delta_reference_frame);
    let rotation_delta = rotation_delta_expr(path);
    as_struct(vec![translation_delta, rotation_delta]).alias(DELTA_FIELD)
}

/// Configuration for the `DeltaTransformEnricher` stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeltaTransformEnricherConfig {
    /// A list of topic names to be processed. These topics should contain nested
    /// transform structs, such as the output of `TfChainEnricher`.
    pub topic_names: Vec<String>,
    /// Reference frame used for `delta_transform.translation`.
    ///
    /// `previous_target_frame` is the body-frame action delta:
    /// `inverse(R_previous) * (p_current - p_previous)`.
    /// `source_frame` computes source-frame coordinate component deltas.
    pub delta_reference_frame: DeltaReferenceFrame,
}

impl DeltaTransformEnricherConfig {
    pub fn new<T: Into<String>>(
        topic_names: Vec<T>,
        delta_reference_frame: DeltaReferenceFrame,
    ) -> Self {
        Self {
            topic_names: topic_names.into_iter().map(Into::into).collect(),
            delta_reference_frame,
        }
    }
}

#[typetag::serde(name = "DeltaTransformEnricherConfig")]
impl StageConfig for DeltaTransformEnricherConfig {
    fn build(&self) -> Box<dyn Stage> {
        Box::new(DeltaTransformEnricher::new(self.clone()))
    }
}

/// An enricher that calculates the change (delta) in transformations between consecutive messages.
///
/// This stage inspects the schema of the DataFrames for the topics specified in the
/// `DeltaTransformEnricherConfig`. It recursively searches for any struct named `transform`
/// that contains `translation` (Vector3) and `rotation` (Quaternion) fields.
///
/// For each `transform` struct found, it computes the delta between the current and previous
/// timestamp and adds it as a new field named `delta_transform`.
///
/// `delta_reference_frame` controls the translation delta:
/// - `previous_target_frame`: `inverse(R_previous) * (p_current - p_previous)`
/// - `source_frame`: legacy source-frame component difference
///
/// Rotation delta preserves the existing relative quaternion behavior.
///
/// # Preconditions
///
/// - `dataset`: **Required** (topics should contain nested `transform` structs, typically from `TfChainEnricher`)
///
/// # Postconditions
///
/// - `dataset`: **Guaranteed** (specified topics enriched with `delta_transform` field in each transform struct)
///
/// Note: Topics without valid transform structs are silently skipped.
///
/// # Errors
///
/// - [`StageError::MissingData`]: `dataset` not set in context
pub struct DeltaTransformEnricher {
    topic_names: Vec<String>,
    delta_reference_frame: DeltaReferenceFrame,
}

impl DeltaTransformEnricher {
    pub fn new(config: DeltaTransformEnricherConfig) -> Self {
        Self {
            topic_names: config.topic_names,
            delta_reference_frame: config.delta_reference_frame,
        }
    }
}

impl Stage for DeltaTransformEnricher {
    fn name(&self) -> &'static str {
        "delta_transform_enricher"
    }

    fn run(&mut self, mut context: Context) -> Result<Context, StageError> {
        let dataset = context.dataset_mut().or_missing("dataset in context")?;

        for topic in &self.topic_names {
            let Some(frame) = dataset.get_mut(topic) else {
                continue;
            };

            let mut schema_probe = frame.clone();
            let schema = schema_probe.collect_schema()?;
            let inspector = TransformSchemaInspector::new(schema.as_ref());
            let paths = inspector.discover_paths();
            if paths.is_empty() {
                continue;
            }

            let mut current_frame = frame.clone();
            for path in paths {
                let expr = path.build_expr(self.delta_reference_frame);
                current_frame = current_frame.with_columns(vec![expr]);
            }
            *frame = current_frame;
        }

        Ok(context)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::HashMap;

    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    use super::*;
    use crate::enrich::tf_buffer_enricher::TfBufferEnricherConfig;
    use crate::enrich::tf_chain_enricher::{FramePair, TfChainEnricherConfig};
    use crate::ingest::rosbag2_ingestor::{Rosbag2Ingestor, Rosbag2IngestorConfig};
    use crate::testutil::{McapGenerator, McapGeneratorConfig};

    fn build_transform_frame(
        translations: &[(f64, f64, f64)],
        rotations_xyzw: &[(f64, f64, f64, f64)],
    ) -> LazyFrame {
        let tx = translations
            .iter()
            .map(|(x, _, _)| *x)
            .collect::<Vec<_>>();
        let ty = translations
            .iter()
            .map(|(_, y, _)| *y)
            .collect::<Vec<_>>();
        let tz = translations
            .iter()
            .map(|(_, _, z)| *z)
            .collect::<Vec<_>>();
        let qx = rotations_xyzw
            .iter()
            .map(|(x, _, _, _)| *x)
            .collect::<Vec<_>>();
        let qy = rotations_xyzw
            .iter()
            .map(|(_, y, _, _)| *y)
            .collect::<Vec<_>>();
        let qz = rotations_xyzw
            .iter()
            .map(|(_, _, z, _)| *z)
            .collect::<Vec<_>>();
        let qw = rotations_xyzw
            .iter()
            .map(|(_, _, _, w)| *w)
            .collect::<Vec<_>>();

        df! {
            "tx" => tx,
            "ty" => ty,
            "tz" => tz,
            "qx" => qx,
            "qy" => qy,
            "qz" => qz,
            "qw" => qw,
        }
        .unwrap()
        .lazy()
        .select([as_struct(vec![as_struct(vec![as_struct(vec![
            as_struct(vec![
                col("tx").alias("x"),
                col("ty").alias("y"),
                col("tz").alias("z"),
            ])
            .alias("translation"),
            as_struct(vec![
                col("qx").alias("x"),
                col("qy").alias("y"),
                col("qz").alias("z"),
                col("qw").alias("w"),
            ])
            .alias("rotation"),
        ])
        .alias("transform")])
        .alias("arm_link")])
        .alias("base_link")])
    }

    fn run_delta_frame(
        frame: LazyFrame,
        delta_reference_frame: DeltaReferenceFrame,
    ) -> DataFrame {
        let mut dataset = HashMap::new();
        dataset.insert("/tf_chain".to_string(), frame);
        let context = Context::new(dataset);
        let mut enricher =
            DeltaTransformEnricherConfig::new(vec!["/tf_chain"], delta_reference_frame).build();
        let context = enricher.run(context).unwrap();
        context
            .dataset
            .unwrap()
            .get("/tf_chain")
            .unwrap()
            .clone()
            .collect()
            .unwrap()
    }

    fn delta_translation_at(df: &DataFrame, row: usize) -> (f64, f64, f64) {
        let base_link = df.column("base_link").unwrap().struct_().unwrap().clone();
        let arm_link_series = base_link.field_by_name("arm_link").unwrap().clone();
        let arm_link = arm_link_series.struct_().unwrap().clone();
        let delta_transform_series = arm_link.field_by_name(DELTA_FIELD).unwrap().clone();
        let delta_transform = delta_transform_series.struct_().unwrap().clone();
        let translation_series = delta_transform
            .field_by_name("translation")
            .unwrap()
            .clone();
        let translation = translation_series.struct_().unwrap().clone();
        let x = translation
            .field_by_name("x")
            .unwrap()
            .f64()
            .unwrap()
            .get(row)
            .unwrap();
        let y = translation
            .field_by_name("y")
            .unwrap()
            .f64()
            .unwrap()
            .get(row)
            .unwrap();
        let z = translation
            .field_by_name("z")
            .unwrap()
            .f64()
            .unwrap()
            .get(row)
            .unwrap();
        (x, y, z)
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn previous_target_frame_rotates_translation_delta_by_previous_target_inverse() {
        let half_sqrt = std::f64::consts::FRAC_1_SQRT_2;
        let frame = build_transform_frame(
            &[(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)],
            &[(0.0, 0.0, half_sqrt, half_sqrt), (0.0, 0.0, half_sqrt, half_sqrt)],
        );

        let df = run_delta_frame(frame, DeltaReferenceFrame::PreviousTargetFrame);
        let (x0, y0, z0) = delta_translation_at(&df, 0);
        assert_close(x0, 0.0);
        assert_close(y0, 0.0);
        assert_close(z0, 0.0);

        let (x1, y1, z1) = delta_translation_at(&df, 1);
        assert_close(x1, 0.0);
        assert_close(y1, -1.0);
        assert_close(z1, 0.0);
    }

    #[test]
    fn source_frame_preserves_legacy_source_component_translation_delta() {
        let half_sqrt = std::f64::consts::FRAC_1_SQRT_2;
        let frame = build_transform_frame(
            &[(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)],
            &[(0.0, 0.0, half_sqrt, half_sqrt), (0.0, 0.0, half_sqrt, half_sqrt)],
        );

        let df = run_delta_frame(frame, DeltaReferenceFrame::SourceFrame);
        let (x1, y1, z1) = delta_translation_at(&df, 1);
        assert_close(x1, 1.0);
        assert_close(y1, 0.0);
        assert_close(z1, 0.0);
    }

    #[test]
    fn delta_reference_frame_is_required_in_config() {
        let yaml = r#"
topic_names:
  - /tf_chain
"#;

        let result = serde_yaml::from_str::<DeltaTransformEnricherConfig>(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn enriches_transform_columns_with_delta() {
        // Generate synthetic MCAP with TF data
        let dir = tempdir().unwrap();
        let output_dir = Utf8PathBuf::try_from(dir.path().to_path_buf()).unwrap();

        let config = McapGeneratorConfig {
            num_frames: 20,
            fps: 10,
            base_frame: "base_link".to_string(),
            child_frames: vec!["arm_link".to_string()],
            generate_images: false,
            generate_tf: true,
            ..Default::default()
        };

        let generator = McapGenerator::new(config);
        let mcap_path = generator.generate(&output_dir).unwrap();

        // Ingest the MCAP
        let mut context = Context::default();
        context.set_rosbag_path(mcap_path);
        let mut ingestor = Rosbag2Ingestor::new(Rosbag2IngestorConfig::without_metadata());
        let mut context = ingestor.run(context).unwrap();

        // Run TF buffer enricher
        let mut tf_buffer = TfBufferEnricherConfig::new().build();
        context = tf_buffer.run(context).unwrap();

        // Run TF chain enricher
        let frame_pairs = vec![FramePair {
            source: "base_link".to_string(),
            target: "arm_link".to_string(),
        }];
        let mut tf_chain = TfChainEnricherConfig::new(frame_pairs).build();
        context = tf_chain.run(context).unwrap();

        // Run delta transform enricher
        let mut enricher = DeltaTransformEnricherConfig::new(
            vec!["/tf_chain"],
            DeltaReferenceFrame::PreviousTargetFrame,
        )
        .build();
        let context = enricher.run(context).unwrap();
        let dataset = context.dataset.unwrap();
        let df = dataset.get("/tf_chain").unwrap().clone().collect().unwrap();

        let base_link = df.column("base_link").unwrap().struct_().unwrap().clone();
        let arm_link_series = base_link.field_by_name("arm_link").unwrap().clone();
        let arm_link = arm_link_series.struct_().unwrap().clone();
        assert!(
            arm_link.field_by_name(DELTA_FIELD).is_ok(),
            "transform parent struct should contain delta_transform alongside transform"
        );

        assert!(
            arm_link.field_by_name("transform").is_ok(),
            "transform field should remain available"
        );
    }
}
