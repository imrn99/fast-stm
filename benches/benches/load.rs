use atomic::Atomic;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use fast_stm::TVar;

#[allow(unused)]
#[derive(Debug, Clone, Copy)]
struct Vertex(pub f64, pub f64, pub f64);

unsafe impl bytemuck::Zeroable for Vertex {}
unsafe impl bytemuck::Pod for Vertex {}

pub fn criterion_benchmark(c: &mut Criterion) {
    // Atomic load time
    let at_bool = black_box(AtomicBool::new(false));
    let at_u32 = black_box(AtomicU32::new(21123));
    let at_struct = black_box(Atomic::new(Vertex(1.0, 2.5, 4.9)));

    let mut ref1 = c.benchmark_group("atomic-load");
    ref1.bench_function("bool", |b| {
        b.iter(|| black_box(at_bool.load(Ordering::Relaxed)))
    });
    ref1.bench_function("u32", |b| {
        b.iter(|| black_box(at_u32.load(Ordering::Relaxed)))
    });
    ref1.bench_function("struct", |b| {
        b.iter(|| black_box(at_struct.load(Ordering::Relaxed)))
    });
    ref1.finish();

    // TVar load time
    let tv_bool = black_box(TVar::new(false));
    let tv_u32 = black_box(TVar::new(21123_u32));
    let tv_struct = black_box(TVar::new(Vertex(1.0, 2.5, 4.9)));

    let mut g1 = c.benchmark_group("tvar-load");
    g1.bench_function("bool", |b| b.iter(|| black_box(tv_bool.read_atomic())));
    g1.bench_function("u32", |b| b.iter(|| black_box(tv_u32.read_atomic())));
    g1.bench_function("struct", |b| b.iter(|| black_box(tv_struct.read_atomic())));
    g1.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
