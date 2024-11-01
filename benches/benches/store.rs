use atomic::Atomic;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use fast_stm::{atomically, TVar};

#[allow(unused)]
#[derive(Debug, Clone, Copy)]
struct Vertex(pub f64, pub f64, pub f64);

unsafe impl bytemuck::Zeroable for Vertex {}
unsafe impl bytemuck::Pod for Vertex {}

pub fn criterion_benchmark(c: &mut Criterion) {
    // Atomic store time
    let at_bool = black_box(AtomicBool::new(false));
    let at_u32 = black_box(AtomicU32::new(21123));
    let at_struct = black_box(Atomic::new(Vertex(1.0, 2.5, 4.9)));

    let mut ref1 = c.benchmark_group("atomic-store");
    ref1.bench_function("bool", |b| {
        b.iter(|| {
            at_bool.store(black_box(true), Ordering::Relaxed);
            black_box(&at_bool)
        })
    });
    ref1.bench_function("u32", |b| {
        b.iter(|| {
            at_u32.store(black_box(21424), Ordering::Relaxed);
            black_box(&at_u32)
        })
    });
    ref1.bench_function("struct", |b| {
        b.iter(|| {
            at_struct.store(black_box(Vertex(2.0, 1.0, 3.1)), Ordering::Relaxed);
            black_box(&at_struct)
        })
    });
    ref1.finish();

    // TVar store time
    let tv_bool = black_box(TVar::new(false));
    let tv_u32 = black_box(TVar::new(21123_u32));
    let tv_struct = black_box(TVar::new(Vertex(1.0, 2.5, 4.9)));

    let mut g1 = c.benchmark_group("tvar-store");
    g1.bench_function("bool", |b| {
        b.iter(|| {
            atomically(|trans| tv_bool.write(trans, black_box(true)));
            black_box(&tv_bool)
        })
    });
    g1.bench_function("u32", |b| {
        b.iter(|| {
            atomically(|trans| tv_u32.write(trans, black_box(21424)));
            black_box(&tv_u32)
        })
    });
    g1.bench_function("struct", |b| {
        b.iter(|| {
            atomically(|trans| tv_struct.write(trans, black_box(Vertex(2.0, 1.0, 3.1))));
            black_box(&tv_struct)
        })
    });
    g1.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
