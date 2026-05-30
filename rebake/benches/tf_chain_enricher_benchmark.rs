#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::collections::HashMap;

use camino::Utf8PathBuf;
use criterion::{Criterion, criterion_group, criterion_main};
use polars::prelude::LazyFrame;

use rebake::core::StageConfig;
use rebake::core::stage::{Context, Stage};
use rebake::enrich::tf_buffer_enricher::TfBufferEnricherConfig;
use rebake::enrich::tf_chain_enricher::{FramePair, TfChainEnricherConfig};
use rebake::ingest::rosbag1_ingestor::{Rosbag1Ingestor, Rosbag1IngestorConfig};

fn build_tf_buffer_dataset() -> HashMap<String, LazyFrame> {
    let manifest_dir = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let rosbag_path = manifest_dir.join("../airoa-moma-raw/000730/data.bag");

    let mut context = Context::default();
    context.set_rosbag_path(rosbag_path);

    let mut ingestor = Rosbag1Ingestor::new(Rosbag1IngestorConfig::new());
    let context = ingestor
        .run(context)
        .expect("failed to ingest rosbag for tf_chain benchmark");

    let mut tf_buffer = TfBufferEnricherConfig::new().build();
    let context = tf_buffer
        .run(context)
        .expect("failed to build tf_buffer dataset for benchmark");

    context
        .dataset
        .expect("tf_buffer enricher must set dataset in context")
}

fn tf_chain_enricher_benchmark(c: &mut Criterion) {
    let base_dataset = build_tf_buffer_dataset();
    let frame_pairs = vec![
        FramePair {
            source: "base_link".to_string(),
            target: "hand_palm_link".to_string(),
        },
        FramePair {
            source: "base_link".to_string(),
            target: "odom".to_string(),
        },
        FramePair {
            source: "arm_lift_link".to_string(),
            target: "arm_roll_link".to_string(),
        },
    ];
    let mut enricher = TfChainEnricherConfig::new(frame_pairs).build();

    c.bench_function("tf_chain_enricher", |b| {
        b.iter(|| {
            let dataset_clone = base_dataset.clone();
            let context = Context::new(dataset_clone);
            enricher
                .run(context)
                .expect("tf_chain enricher benchmark iteration failed");
        });
    });
}

criterion_group!(benches, tf_chain_enricher_benchmark);
criterion_main!(benches);
