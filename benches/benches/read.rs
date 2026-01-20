use std::hint::black_box;
use std::sync::atomic::{AtomicU32, Ordering};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use fast_stm::{atomically, init_transaction, TVar};

/// Read routines benchmarks
///
/// Compare:
/// - `TVar::read` in transaction
/// - `TVar::read` alone
/// - `TVar::read_atomic`
/// - `Atomic::load`
pub fn criterion_benchmark(c: &mut Criterion) {
    let tvar = TVar::new(42_u32);
    let atom = AtomicU32::new(42);

    let mut g1 = c.benchmark_group("read-times");
    g1.bench_function("TVar::<u32>::read (alone)", |b| {
        let mut tx = init_transaction();
        b.iter(|| black_box(tx.read(&tvar)))
    });
    g1.bench_function("TVar::<u32>::read (in transaction)", |b| {
        b.iter(|| black_box(atomically(|t| tvar.read(t))))
    });
    g1.bench_function("TVar::<u32>::read_atomic", |b| {
        b.iter(|| black_box(tvar.read_atomic()))
    });
    g1.bench_function("AtomicU32::load", |b| {
        b.iter(|| black_box(atom.load(Ordering::Relaxed)))
    });
    g1.finish();

    let n_reads = [1_000, 10_000, 100_000, 1_000_000];
    let tvars: Vec<_> = (0..*n_reads.last().unwrap())
        .map(|_| TVar::new(42_u32))
        .collect();
    let mut g2 = c.benchmark_group("read-times-vs-n-reads");
    for n_read in n_reads {
        g2.throughput(Throughput::Elements(n_read));
        g2.bench_with_input(
            BenchmarkId::new("TVar::<u32>::read", n_read),
            &(n_read, &tvars),
            |b, &(n, tvs)| {
                let mut tx = init_transaction();
                b.iter(|| {
                    for i in 0..n {
                        #[allow(unused_must_use)]
                        black_box(tx.read(&tvs[i as usize]));
                    }
                })
            },
        );
    }
    g2.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
