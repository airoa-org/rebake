use std::collections::{HashMap, HashSet};

use polars_arrow::array::{BooleanArray, Float64Array, StructArray, Utf8ViewArray};
extern crate nalgebra as na;

use polars::prelude::*;

use crate::core::error::StageResult;
use crate::core::stage::StageError;

type FrameId = u32;
type Edge = (FrameId, FrameId);

#[derive(Debug, Default, Clone, PartialEq)]
struct FrameIndexer {
    name_to_id: HashMap<String, FrameId>,
    names: Vec<String>,
}

impl FrameIndexer {
    fn new() -> Self {
        Self {
            name_to_id: HashMap::new(),
            names: Vec::new(),
        }
    }

    fn intern(&mut self, name: &str) -> FrameId {
        if let Some(&id) = self.name_to_id.get(name) {
            return id;
        }

        let id = self.names.len() as FrameId;
        let owned = name.to_string();
        self.names.push(owned.clone());
        self.name_to_id.insert(owned, id);
        id
    }

    fn id(&self, name: &str) -> Option<FrameId> {
        self.name_to_id.get(name).copied()
    }

    fn name(&self, id: FrameId) -> Option<&str> {
        self.names.get(id as usize).map(String::as_str)
    }

    fn len(&self) -> usize {
        self.names.len()
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Frame {
    parent: Option<FrameId>,
    children: Vec<FrameId>,
}

impl Frame {
    fn set_parent(&mut self, parent: FrameId) {
        self.parent = Some(parent);
    }

    fn add_child(&mut self, child: FrameId) {
        self.children.push(child);
    }

    fn parent(&self) -> Option<FrameId> {
        self.parent
    }

    fn reset(&mut self) {
        self.parent = None;
        self.children.clear();
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct TFChain {
    frame_indexer: FrameIndexer,
    frames: Vec<Frame>,
    transforms: HashMap<(FrameId, FrameId), na::Isometry3<f64>>,
    freshness: HashMap<(FrameId, FrameId), bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChainComputation {
    pub transform: na::Isometry3<f64>,
    pub is_fresh: bool,
}

impl TFChain {
    fn with_indexer(indexer: FrameIndexer) -> Self {
        let frame_count = indexer.len();
        Self {
            frame_indexer: indexer,
            frames: vec![Frame::default(); frame_count],
            transforms: HashMap::new(),
            freshness: HashMap::new(),
        }
    }

    pub fn forward_kinematics_with_freshness(
        &self,
        source_frame_id: &str,
        target_frame_id: &str,
    ) -> StageResult<ChainComputation> {
        let source_id = self.resolve_frame_id(source_frame_id)?;
        let target_id = self.resolve_frame_id(target_frame_id)?;

        if source_id == target_id {
            return Ok(ChainComputation {
                transform: na::Isometry3::identity(),
                is_fresh: false,
            });
        }

        let source_ancestry = self.ancestor_chain_to_root(source_id)?;
        let target_ancestry = self.ancestor_chain_to_root(target_id)?;
        let (source_lca_idx, target_lca_idx) = self
            .find_lowest_common_ancestor(&source_ancestry, &target_ancestry)
            .ok_or_else(|| {
                StageError::invalid(format!(
                    "frames '{}' and '{}' do not share a common ancestor",
                    source_frame_id, target_frame_id
                ))
            })?;

        let lca_to_source = self.compose_downward_path(&source_ancestry, source_lca_idx)?;
        let lca_to_target = self.compose_downward_path(&target_ancestry, target_lca_idx)?;

        Ok(ChainComputation {
            transform: lca_to_source.transform.inverse() * lca_to_target.transform,
            is_fresh: lca_to_source.is_fresh || lca_to_target.is_fresh,
        })
    }

    pub(crate) fn reset(&mut self) {
        for frame in &mut self.frames {
            frame.reset();
        }
        self.transforms.clear();
        self.freshness.clear();
    }

    fn resolve_frame_id(&self, name: &str) -> StageResult<FrameId> {
        self.frame_indexer.id(name).ok_or_else(|| {
            StageError::invalid(format!("frame '{}' is not present in tf_buffer", name))
        })
    }

    fn ancestor_chain_to_root(&self, start: FrameId) -> StageResult<Vec<FrameId>> {
        let mut ancestry = vec![start];
        let mut current = start;
        let mut visited = HashSet::from([start]);

        while let Some(parent) = self
            .frames
            .get(current as usize)
            .ok_or_else(|| {
                StageError::invalid(format!(
                    "frame id {} is out of bounds for TF chain ancestry traversal",
                    current
                ))
            })?
            .parent()
        {
            if !visited.insert(parent) {
                let frame_name = self.frame_indexer.name(parent).unwrap_or("<unknown>");
                return Err(StageError::invalid(format!(
                    "cycle detected in TF tree while traversing ancestor '{}'",
                    frame_name
                )));
            }
            ancestry.push(parent);
            current = parent;
        }

        Ok(ancestry)
    }

    fn find_lowest_common_ancestor(
        &self,
        source_ancestry: &[FrameId],
        target_ancestry: &[FrameId],
    ) -> Option<(usize, usize)> {
        let source_positions: HashMap<FrameId, usize> = source_ancestry
            .iter()
            .enumerate()
            .map(|(idx, frame_id)| (*frame_id, idx))
            .collect();

        target_ancestry
            .iter()
            .enumerate()
            .find_map(|(target_idx, frame_id)| {
                source_positions
                    .get(frame_id)
                    .copied()
                    .map(|source_idx| (source_idx, target_idx))
            })
    }

    fn compose_downward_path(
        &self,
        ancestry: &[FrameId],
        ancestor_index: usize,
    ) -> StageResult<ChainComputation> {
        let mut transform = na::Isometry3::identity();
        let mut is_fresh = false;

        for idx in (0..ancestor_index).rev() {
            let parent = ancestry[idx + 1];
            let child = ancestry[idx];
            let edge = (parent, child);
            let edge_transform = self.transforms.get(&edge).ok_or_else(|| {
                StageError::invalid(format!(
                    "missing transform for edge '{} -> {}' in TF chain",
                    self.frame_indexer.name(parent).unwrap_or("<unknown>"),
                    self.frame_indexer.name(child).unwrap_or("<unknown>")
                ))
            })?;
            transform *= edge_transform;
            is_fresh |= self.freshness.get(&edge).copied().unwrap_or(false);
        }

        Ok(ChainComputation {
            transform,
            is_fresh,
        })
    }

    fn set_parent(&mut self, child: FrameId, parent: FrameId) -> StageResult<()> {
        let child_name = self.frame_indexer.name(child).unwrap_or("<unknown>");
        let parent_name = self.frame_indexer.name(parent).unwrap_or("<unknown>");

        let frame = self.frames.get_mut(child as usize).ok_or_else(|| {
            StageError::invalid(format!(
                "frame '{}' is out of bounds while building TF chain",
                child_name
            ))
        })?;

        if let Some(existing_parent) = frame.parent() {
            if existing_parent != parent {
                let existing_parent_name = self
                    .frame_indexer
                    .name(existing_parent)
                    .unwrap_or("<unknown>");
                return Err(StageError::invalid(format!(
                    "frame '{}' has multiple parents in the same tf_buffer row: '{}' and '{}'",
                    child_name, existing_parent_name, parent_name
                )));
            }
        }

        frame.set_parent(parent);
        Ok(())
    }

    fn add_child(&mut self, parent: FrameId, child: FrameId) {
        if let Some(frame) = self.frames.get_mut(parent as usize) {
            frame.add_child(child);
        }
    }

    fn insert_transform(
        &mut self,
        parent: FrameId,
        child: FrameId,
        transform: na::Isometry3<f64>,
        is_fresh: bool,
    ) {
        self.transforms.insert((parent, child), transform);
        self.freshness.insert((parent, child), is_fresh);
    }
}

#[derive(Debug)]
pub(crate) struct TfChainBuilder {
    frame_pairs: Vec<(String, String)>,
    required_edges: HashSet<(FrameId, FrameId)>,
    tf_chain: TFChain,
}

impl TfChainBuilder {
    /// Initializes a new TfChainBuilder from frame pairs and a transforms series.
    ///
    /// # Panics
    ///
    /// Panics if the series is not a struct type (caller must provide valid TF buffer data).
    pub fn initialize(
        frame_pairs: &[(&str, &str)],
        series: &Series,
    ) -> StageResult<(Self, Vec<ChainComputation>)> {
        let struct_chunked = series
            .struct_()
            .map_err(|e| StageError::invalid_with("transforms series must be a struct", e))?;

        let total_len = series.len();
        let mut frame_indexer = FrameIndexer::new();
        let mut parent_map: HashMap<FrameId, HashSet<FrameId>> = HashMap::with_capacity(total_len);

        for struct_array in struct_chunked.downcast_iter() {
            let child_array = column_as_utf8(struct_array, "child_frame_id");
            let header_array = column_as_struct(struct_array, "header");
            let parent_array = column_as_utf8(header_array, "frame_id");

            for i in 0..struct_array.len() {
                let child_id = frame_indexer.intern(child_array.value(i));
                let parent_id = frame_indexer.intern(parent_array.value(i));
                parent_map.entry(child_id).or_default().insert(parent_id);
            }
        }

        let required_edges = collect_required_edges(&parent_map, frame_pairs, &frame_indexer)?;
        let frame_pairs_vec = frame_pairs
            .iter()
            .map(|(source, target)| ((*source).to_string(), (*target).to_string()))
            .collect();

        let mut builder = TfChainBuilder {
            frame_pairs: frame_pairs_vec,
            required_edges,
            tf_chain: TFChain::with_indexer(frame_indexer),
        };

        let transforms = builder.populate_tf_chain(series)?;
        Ok((builder, transforms))
    }

    pub fn update(&mut self, series: &Series) -> StageResult<Vec<ChainComputation>> {
        self.populate_tf_chain(series)
    }

    fn populate_tf_chain(&mut self, series: &Series) -> StageResult<Vec<ChainComputation>> {
        self.tf_chain.reset();

        let struct_chunked = series
            .struct_()
            .map_err(|e| StageError::invalid_with("transforms series must be a struct", e))?;

        for struct_array in struct_chunked.downcast_iter() {
            let child_array = column_as_utf8(struct_array, "child_frame_id");
            let header_array = column_as_struct(struct_array, "header");
            let parent_array = column_as_utf8(header_array, "frame_id");
            let transform_array = column_as_struct(struct_array, "transform");
            let freshness_array = column_as_bool(struct_array, "is_fresh");
            let translation_array = column_as_struct(transform_array, "translation");
            let rotation_array = column_as_struct(transform_array, "rotation");

            let tx_array = column_as_f64(translation_array, "x");
            let ty_array = column_as_f64(translation_array, "y");
            let tz_array = column_as_f64(translation_array, "z");
            let qx_array = column_as_f64(rotation_array, "x");
            let qy_array = column_as_f64(rotation_array, "y");
            let qz_array = column_as_f64(rotation_array, "z");
            let qw_array = column_as_f64(rotation_array, "w");

            for i in 0..struct_array.len() {
                let child_name = child_array.value(i);
                let parent_name = parent_array.value(i);

                let child_id = self.tf_chain.frame_indexer.id(child_name).ok_or_else(|| {
                    StageError::invalid(format!(
                        "child frame '{}' was not registered in TF chain indexer",
                        child_name
                    ))
                })?;
                let parent_id = self.tf_chain.frame_indexer.id(parent_name).ok_or_else(|| {
                    StageError::invalid(format!(
                        "parent frame '{}' was not registered in TF chain indexer",
                        parent_name
                    ))
                })?;

                let edge = (parent_id, child_id);
                if !self.required_edges.contains(&edge) {
                    continue;
                }

                let translation =
                    na::Vector3::new(tx_array.value(i), ty_array.value(i), tz_array.value(i));
                let orientation = na::Quaternion::new(
                    qw_array.value(i),
                    qx_array.value(i),
                    qy_array.value(i),
                    qz_array.value(i),
                );

                let transform = na::Isometry3::from_parts(
                    translation.into(),
                    na::UnitQuaternion::new_normalize(orientation),
                );

                self.tf_chain.set_parent(child_id, parent_id)?;
                self.tf_chain.add_child(parent_id, child_id);
                self.tf_chain.insert_transform(
                    parent_id,
                    child_id,
                    transform,
                    freshness_array.value(i),
                );
            }
        }

        let mut results = Vec::with_capacity(self.frame_pairs.len());
        for (source, target) in &self.frame_pairs {
            results.push(
                self.tf_chain
                    .forward_kinematics_with_freshness(source, target)?,
            );
        }
        Ok(results)
    }
}

fn collect_required_edges(
    parent_map: &HashMap<FrameId, HashSet<FrameId>>,
    frame_pairs: &[(&str, &str)],
    indexer: &FrameIndexer,
) -> StageResult<HashSet<Edge>> {
    let mut edges = HashSet::new();

    for (source_name, target_name) in frame_pairs {
        let source_id = indexer.id(source_name).ok_or_else(|| {
            StageError::invalid(format!(
                "source frame '{}' is not present in tf_buffer",
                source_name
            ))
        })?;
        let target_id = indexer.id(target_name).ok_or_else(|| {
            StageError::invalid(format!(
                "target frame '{}' is not present in tf_buffer",
                target_name
            ))
        })?;

        edges.extend(collect_all_upstream_edges(parent_map, source_id));
        edges.extend(collect_all_upstream_edges(parent_map, target_id));
    }

    Ok(edges)
}

fn collect_all_upstream_edges(
    parent_map: &HashMap<FrameId, HashSet<FrameId>>,
    start_node: FrameId,
) -> HashSet<Edge> {
    let mut edges = HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(start_node);
    let mut visited = HashSet::new();
    visited.insert(start_node);

    while let Some(current) = queue.pop_front() {
        if let Some(parents) = parent_map.get(&current) {
            for &parent in parents {
                edges.insert((parent, current));
                if !visited.contains(&parent) {
                    visited.insert(parent);
                    queue.push_back(parent);
                }
            }
        }
    }
    edges
}

/// Extracts a nested struct column from a StructArray by name.
///
/// # Panics
///
/// Panics if the column doesn't exist or is not a struct type.
/// Caller must ensure the schema matches expected TF buffer format.
fn column_as_struct<'a>(array: &'a StructArray, name: &str) -> &'a StructArray {
    let idx = column_index(array, name);
    // CONTRACT: TF buffer schema guarantees this column is a struct
    #[allow(clippy::expect_used)]
    array.values()[idx]
        .as_any()
        .downcast_ref::<StructArray>()
        .expect("column must be a struct type")
}

/// Extracts a UTF8 string column from a StructArray by name.
///
/// # Panics
///
/// Panics if the column doesn't exist or is not a UTF8 type.
/// Caller must ensure the schema matches expected TF buffer format.
fn column_as_utf8<'a>(array: &'a StructArray, name: &str) -> &'a Utf8ViewArray {
    let idx = column_index(array, name);
    // CONTRACT: TF buffer schema guarantees this column is UTF8
    #[allow(clippy::expect_used)]
    array.values()[idx]
        .as_any()
        .downcast_ref::<Utf8ViewArray>()
        .expect("column must be a Utf8View type")
}

/// Extracts a Float64 column from a StructArray by name.
///
/// # Panics
///
/// Panics if the column doesn't exist or is not a Float64 type.
/// Caller must ensure the schema matches expected TF buffer format.
fn column_as_f64<'a>(array: &'a StructArray, name: &str) -> &'a Float64Array {
    let idx = column_index(array, name);
    // CONTRACT: TF buffer schema guarantees this column is Float64
    #[allow(clippy::expect_used)]
    array.values()[idx]
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("column must be a Float64 type")
}

/// Extracts a Boolean column from a StructArray by name.
///
/// # Panics
///
/// Panics if the column doesn't exist or is not a Boolean type.
/// Caller must ensure the schema matches expected TF buffer format.
fn column_as_bool<'a>(array: &'a StructArray, name: &str) -> &'a BooleanArray {
    let idx = column_index(array, name);
    // CONTRACT: TF buffer schema guarantees this column is Boolean
    #[allow(clippy::expect_used)]
    array.values()[idx]
        .as_any()
        .downcast_ref::<BooleanArray>()
        .expect("column must be a Boolean type")
}

/// Returns the index of a column by name in a StructArray.
///
/// # Panics
///
/// Panics if the column doesn't exist. Caller must ensure the schema
/// matches expected TF buffer format.
fn column_index(array: &StructArray, name: &str) -> usize {
    array
        .fields()
        .iter()
        .position(|field| field.name.as_str() == name)
        .unwrap_or_else(|| panic!("column '{}' must exist in struct", name))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use polars::prelude::StructChunked;

    /// Creates a mock TF buffer Series with per-edge translation, rotation, and freshness.
    fn create_tf_buffer_series_with_translation(
        entries: &[(&str, &str, (f64, f64, f64), (f64, f64, f64, f64), bool)],
    ) -> Series {
        let len = entries.len();
        let child_frame_id = Series::new(
            "child_frame_id".into(),
            entries
                .iter()
                .map(|(_, child, _, _, _)| *child)
                .collect::<Vec<_>>(),
        );
        let parent_frame_id = Series::new(
            "frame_id".into(),
            entries
                .iter()
                .map(|(parent, _, _, _, _)| *parent)
                .collect::<Vec<_>>(),
        );

        let header = StructChunked::from_series("header".into(), len, [parent_frame_id].iter())
            .unwrap()
            .into_series();

        let tx = Series::new(
            "x".into(),
            entries
                .iter()
                .map(|(_, _, translation, _, _)| translation.0)
                .collect::<Vec<_>>(),
        );
        let ty = Series::new(
            "y".into(),
            entries
                .iter()
                .map(|(_, _, translation, _, _)| translation.1)
                .collect::<Vec<_>>(),
        );
        let tz = Series::new(
            "z".into(),
            entries
                .iter()
                .map(|(_, _, translation, _, _)| translation.2)
                .collect::<Vec<_>>(),
        );
        let translation_struct =
            StructChunked::from_series("translation".into(), len, [tx, ty, tz].iter())
                .unwrap()
                .into_series();

        // ROS quaternion format: [x, y, z, w]
        let qx = Series::new(
            "x".into(),
            entries
                .iter()
                .map(|(_, _, _, rotation, _)| rotation.0)
                .collect::<Vec<_>>(),
        );
        let qy = Series::new(
            "y".into(),
            entries
                .iter()
                .map(|(_, _, _, rotation, _)| rotation.1)
                .collect::<Vec<_>>(),
        );
        let qz = Series::new(
            "z".into(),
            entries
                .iter()
                .map(|(_, _, _, rotation, _)| rotation.2)
                .collect::<Vec<_>>(),
        );
        let qw = Series::new(
            "w".into(),
            entries
                .iter()
                .map(|(_, _, _, rotation, _)| rotation.3)
                .collect::<Vec<_>>(),
        );
        let rotation_struct =
            StructChunked::from_series("rotation".into(), len, [qx, qy, qz, qw].iter())
                .unwrap()
                .into_series();

        let transform_struct = StructChunked::from_series(
            "transform".into(),
            len,
            [translation_struct, rotation_struct].iter(),
        )
        .unwrap()
        .into_series();

        let is_fresh = Series::new(
            "is_fresh".into(),
            entries
                .iter()
                .map(|(_, _, _, _, is_fresh)| *is_fresh)
                .collect::<Vec<_>>(),
        );

        StructChunked::from_series(
            "transforms".into(),
            len,
            [child_frame_id, header, transform_struct, is_fresh].iter(),
        )
        .unwrap()
        .into_series()
    }

    /// Creates a mock TF buffer Series with zero translations.
    fn create_tf_buffer_series(entries: &[(&str, &str, (f64, f64, f64, f64), bool)]) -> Series {
        let translated_entries = entries
            .iter()
            .map(|(parent, child, rotation, is_fresh)| {
                (*parent, *child, (0.0, 0.0, 0.0), *rotation, *is_fresh)
            })
            .collect::<Vec<_>>();
        create_tf_buffer_series_with_translation(&translated_entries)
    }

    #[test]
    fn quaternion_component_order() {
        // Test with an arbitrary quaternion to ensure all components are correctly mapped
        // Using a quaternion with all non-zero components
        // ROS format: [x, y, z, w] = [0.1, 0.2, 0.3, 0.9273] (normalized)
        let qx: f64 = 0.1;
        let qy: f64 = 0.2;
        let qz: f64 = 0.3;
        let qw: f64 = 0.9273; // Approximately normalized: sqrt(0.01 + 0.04 + 0.09 + 0.86) ≈ 1

        // Normalize for exact comparison
        let norm = (qx * qx + qy * qy + qz * qz + qw * qw).sqrt();
        let qx_n = qx / norm;
        let qy_n = qy / norm;
        let qz_n = qz / norm;
        let qw_n = qw / norm;

        let series = create_tf_buffer_series(&[(
            "base_link",
            "child_link",
            (qx_n, qy_n, qz_n, qw_n), // ROS [x, y, z, w] format
            true,
        )]);

        let frame_pairs = vec![("base_link", "child_link")];
        let (builder, transforms) = TfChainBuilder::initialize(&frame_pairs, &series).unwrap();
        let _ = builder;

        assert_eq!(transforms.len(), 1);
        let transform = &transforms[0].transform;

        // Verify each quaternion component is correctly mapped
        let q = transform.rotation.quaternion();

        // The key test: verify that ROS [x, y, z, w] maps to nalgebra (w, i, j, k) correctly
        assert!(
            (q.w - qw_n).abs() < 1e-10,
            "w component mismatch: expected {}, got {}",
            qw_n,
            q.w
        );
        assert!(
            (q.i - qx_n).abs() < 1e-10,
            "x component mismatch: expected {}, got {}",
            qx_n,
            q.i
        );
        assert!(
            (q.j - qy_n).abs() < 1e-10,
            "y component mismatch: expected {}, got {}",
            qy_n,
            q.j
        );
        assert!(
            (q.k - qz_n).abs() < 1e-10,
            "z component mismatch: expected {}, got {}",
            qz_n,
            q.k
        );
    }

    #[test]
    fn chain_freshness_is_true_if_any_edge_on_path_is_fresh() {
        let series = create_tf_buffer_series(&[
            ("base_link", "arm_link", (0.0, 0.0, 0.0, 1.0), false),
            ("arm_link", "hand_link", (0.0, 0.0, 0.0, 1.0), true),
        ]);

        let frame_pairs = vec![("base_link", "hand_link")];
        let (mut builder, results) = TfChainBuilder::initialize(&frame_pairs, &series).unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].is_fresh);

        let stale_series = create_tf_buffer_series(&[
            ("base_link", "arm_link", (0.0, 0.0, 0.0, 1.0), false),
            ("arm_link", "hand_link", (0.0, 0.0, 0.0, 1.0), false),
        ]);
        let stale_results = builder.update(&stale_series).unwrap();

        assert_eq!(stale_results.len(), 1);
        assert!(!stale_results[0].is_fresh);
    }

    #[test]
    fn computes_transform_between_sibling_links() {
        let series = create_tf_buffer_series_with_translation(&[
            (
                "base_link",
                "left_tip",
                (1.0, 0.0, 0.0),
                (0.0, 0.0, 0.0, 1.0),
                false,
            ),
            (
                "base_link",
                "right_tip",
                (0.0, 2.0, 0.0),
                (0.0, 0.0, 0.0, 1.0),
                true,
            ),
        ]);

        let frame_pairs = vec![("left_tip", "right_tip")];
        let (_, results) = TfChainBuilder::initialize(&frame_pairs, &series).unwrap();

        assert_eq!(results.len(), 1);
        let translation = results[0].transform.translation.vector;
        assert!((translation.x + 1.0).abs() < 1e-10);
        assert!((translation.y - 2.0).abs() < 1e-10);
        assert!(translation.z.abs() < 1e-10);
        assert!(results[0].is_fresh);
    }

    #[test]
    fn computes_transform_from_descendant_to_ancestor() {
        let series = create_tf_buffer_series_with_translation(&[
            (
                "base_link",
                "arm_link",
                (1.0, 0.0, 0.0),
                (0.0, 0.0, 0.0, 1.0),
                false,
            ),
            (
                "arm_link",
                "hand_link",
                (0.0, 2.0, 0.0),
                (0.0, 0.0, 0.0, 1.0),
                false,
            ),
        ]);

        let frame_pairs = vec![("hand_link", "base_link")];
        let (_, results) = TfChainBuilder::initialize(&frame_pairs, &series).unwrap();

        assert_eq!(results.len(), 1);
        let translation = results[0].transform.translation.vector;
        assert!((translation.x + 1.0).abs() < 1e-10);
        assert!((translation.y + 2.0).abs() < 1e-10);
        assert!(translation.z.abs() < 1e-10);
    }

    #[test]
    fn returns_identity_for_same_source_and_target() {
        let series =
            create_tf_buffer_series(&[("base_link", "hand_link", (0.0, 0.0, 0.0, 1.0), true)]);

        let frame_pairs = vec![("hand_link", "hand_link")];
        let (_, results) = TfChainBuilder::initialize(&frame_pairs, &series).unwrap();

        assert_eq!(results.len(), 1);
        let translation = results[0].transform.translation.vector;
        assert!(translation.x.abs() < 1e-10);
        assert!(translation.y.abs() < 1e-10);
        assert!(translation.z.abs() < 1e-10);
        assert!(!results[0].is_fresh);
    }

    #[test]
    fn errors_when_frames_do_not_share_a_common_ancestor() {
        let series = create_tf_buffer_series_with_translation(&[
            (
                "root_a",
                "left_tip",
                (1.0, 0.0, 0.0),
                (0.0, 0.0, 0.0, 1.0),
                false,
            ),
            (
                "root_b",
                "right_tip",
                (0.0, 2.0, 0.0),
                (0.0, 0.0, 0.0, 1.0),
                false,
            ),
        ]);

        let frame_pairs = vec![("left_tip", "right_tip")];
        let err = TfChainBuilder::initialize(&frame_pairs, &series).unwrap_err();

        assert!(
            err.to_string().contains("do not share a common ancestor"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn errors_when_a_child_has_multiple_parents_in_one_row() {
        let series = create_tf_buffer_series_with_translation(&[
            (
                "root_a",
                "shared_tip",
                (1.0, 0.0, 0.0),
                (0.0, 0.0, 0.0, 1.0),
                false,
            ),
            (
                "root_b",
                "shared_tip",
                (0.0, 2.0, 0.0),
                (0.0, 0.0, 0.0, 1.0),
                false,
            ),
        ]);

        let frame_pairs = vec![("root_a", "shared_tip")];
        let err = TfChainBuilder::initialize(&frame_pairs, &series).unwrap_err();

        assert!(
            err.to_string().contains("multiple parents"),
            "unexpected error: {err}"
        );
    }
}
