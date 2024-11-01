use atomic::Atomic;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::atomic::{AtomicBool, AtomicU32};

use fast_stm::TVar;

#[allow(unused)]
#[derive(Debug, Clone, Copy)]
struct Vertex(pub f64, pub f64, pub f64);

pub fn criterion_benchmark(c: &mut Criterion) {
    // Atomic init time
    let mut ref1 = c.benchmark_group("atomic-init");
    ref1.bench_function("bool", |b| b.iter(|| black_box(AtomicBool::new(false))));
    ref1.bench_function("u32", |b| b.iter(|| black_box(AtomicU32::new(23123))));
    ref1.bench_function("struct", |b| {
        b.iter(|| black_box(Atomic::new(Vertex(1.0, 2.5, 4.9))))
    });
    ref1.finish();

    // TVar init time
    let mut g1 = c.benchmark_group("tvar-init");
    g1.bench_function("bool", |b| b.iter(|| black_box(TVar::new(false))));
    g1.bench_function("u32", |b| b.iter(|| black_box(TVar::new(23123_u32))));
    g1.bench_function("struct", |b| {
        b.iter(|| black_box(TVar::new(Vertex(1.0, 2.5, 4.9))))
    });
    g1.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
