use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use fast_stm::{init_transaction, TVar};

#[allow(unused)]
#[derive(Debug, Clone, Copy)]
struct Vertex(pub f64, pub f64, pub f64);

/// `TVar` initialization routine benchmarks
pub fn tvar_wrapping(c: &mut Criterion) {
    let tbool = TVar::new(false);
    let tu32 = TVar::new(42_u32);
    let tstruct = TVar::new(Vertex(0.0, 1.0, 0.0));
    let theap = TVar::new(String::from("STM!"));

    let mut g1 = c.benchmark_group("tvar-read-times");
    g1.bench_function("TVar::<bool>::read", |b| {
        let mut tx = init_transaction();
        b.iter(|| black_box(tx.read(&tbool)))
    });
    g1.bench_function("TVar::<u32>::read", |b| {
        let mut tx = init_transaction();
        b.iter(|| black_box(tx.read(&tu32)))
    });
    g1.bench_function("TVar::<Vertex>::read", |b| {
        let mut tx = init_transaction();
        b.iter(|| black_box(tx.read(&tstruct)))
    });
    g1.bench_function("TVar::<String>::read", |b| {
        let mut tx = init_transaction();
        b.iter(|| black_box(tx.read(&theap)))
    });
    g1.finish();

    let mut g2 = c.benchmark_group("tvar-write-times");
    g2.bench_function("TVar::<bool>::write", |b| {
        let tbool = TVar::new(false);
        let mut tx = init_transaction();
        b.iter(|| black_box(tx.write(&tbool, true)))
    });
    g2.bench_function("TVar::<u32>::write", |b| {
        let tu32 = TVar::new(42_u32);
        let mut tx = init_transaction();
        b.iter(|| black_box(tx.write(&tu32, 12345)))
    });
    g2.bench_function("TVar::<Vertex>::write", |b| {
        let tstruct = TVar::new(Vertex(0.0, 1.0, 0.0));
        let mut tx = init_transaction();
        b.iter(|| black_box(tx.write(&tstruct, Vertex(1.0, 0.0, 1.0))))
    });
    g2.bench_function("TVar::<String>::write", |b| {
        let theap = TVar::new(String::from("STM!"));
        let mut tx = init_transaction();
        b.iter(|| black_box(tx.write(&theap, String::from("More common than HTM!"))))
    });
    g2.finish();

    let mut g3 = c.benchmark_group("tvar-init-times");
    g3.bench_function("TVar::<bool>::new", |b| {
        b.iter(|| black_box(TVar::new(false)))
    });
    g3.bench_function("TVar::<u32>::new", |b| {
        b.iter(|| black_box(TVar::new(42_u32)))
    });
    g3.bench_function("TVar::<Vertex>::new", |b| {
        b.iter(|| black_box(TVar::new(Vertex(0.0, 1.0, 0.0))))
    });
    g3.bench_function("TVar::<String>::new", |b| {
        b.iter(|| black_box(TVar::new(String::from("STM!"))))
    });
    g3.finish();
}

// TODO: TVar<u32> vs TVarU32
// pub fn tvar_wrapping(c: &mut Criterion) {}

criterion_group!(benches, tvar_wrapping);
criterion_main!(benches);
