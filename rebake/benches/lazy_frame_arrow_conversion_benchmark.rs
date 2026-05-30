#![allow(clippy::unwrap_used, clippy::expect_used)]
use criterion::{Criterion, criterion_group, criterion_main};
use polars::prelude::*;

use rebake::core::{
    lazy_to_record_batch_rechunk, lazy_to_record_batches_iter, record_batch_to_lazy,
};

fn build_lazy_frame() -> LazyFrame {
    let row_count = 200_000;
    let names = (0..row_count)
        .map(|i| format!("name_{i}"))
        .collect::<Vec<_>>();
    let ages = (0..row_count).map(|i| i % 120).collect::<Vec<_>>();
    let heights = (0..row_count)
        .map(|i| 150.0 + (i as f64 * 0.01))
        .collect::<Vec<_>>();

    df! {
        "name" => names,
        "age" => ages,
        "height_cm" => heights,
    }
    .unwrap()
    .lazy()
}

fn lazy_frame_arrow_conversion_benchmark(c: &mut Criterion) {
    let lf = build_lazy_frame();
    let batches = lazy_to_record_batches_iter(&lf);
    let first_batch = batches
        .first()
        .expect("non-empty record batch list")
        .clone();

    c.bench_function("lazy_to_batches_iter_chunks", |b| {
        b.iter(|| {
            let produced = lazy_to_record_batches_iter(&lf);
            assert!(!produced.is_empty());
        })
    });

    c.bench_function("lazy_to_batch_rechunk", |b| {
        b.iter(|| {
            let batch = lazy_to_record_batch_rechunk(&lf);
            assert_eq!(batch.num_rows(), first_batch.num_rows());
        })
    });

    c.bench_function("batch_to_lazy", |b| {
        b.iter(|| {
            let lf = record_batch_to_lazy(&first_batch);
            assert!(lf.clone().collect().is_ok());
        })
    });
}

criterion_group!(benches, lazy_frame_arrow_conversion_benchmark);
criterion_main!(benches);
