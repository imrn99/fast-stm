use std::hint::black_box;
use std::sync::atomic::{AtomicU32, Ordering};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use fast_stm::{atomically, init_transaction, TVar};

/// Write routines benchmarks
///
/// Compare:
/// - `TVar::write` in transaction
/// - `TVar::write` alone
/// - `TVar::write_atomic`
/// - `Atomic::store`
pub fn criterion_benchmark(c: &mut Criterion) {
    let old_value = 42_u32;
    let new_value = 12345;

    let mut g1 = c.benchmark_group("write-times");

    g1.bench_function("TVar::<u32>::write (alone)", |b| {
        let mut tx = init_transaction();
        let tvar = TVar::new(old_value);
        b.iter(|| black_box(tx.write(&tvar, new_value)))
    });
    g1.bench_function("TVar::<u32>::write (in transaction)", |b| {
        let tvar = TVar::new(old_value);
        b.iter(|| atomically(|t| tvar.write(t, new_value)))
    });
    // TODO: uncomment when #25 is merged
    // g1.bench_function("TVar::<u32>::write_atomic", |b| {
    //     let tvar = TVar::new(old_value);
    //     b.iter(|| black_box(tvar.write_atomic(new_value)))
    // });
    g1.bench_function("AtomicU32::store", |b| {
        let atom = AtomicU32::new(old_value);
        b.iter(|| black_box(atom.store(new_value, Ordering::Relaxed)))
    });

    g1.finish();

    let n_writes = [1_000, 10_000, 100_000, 1_000_000];
    let tvars: Vec<_> = (0..*n_writes.last().unwrap())
        .map(|_| TVar::new(old_value))
        .collect();
    let mut g2 = c.benchmark_group("write-times-vs-n-writes");
    for n_write in n_writes {
        g2.throughput(Throughput::Elements(n_write));
        g2.bench_with_input(
            BenchmarkId::new("TVar::<u32>::write", n_write),
            &(n_write, &tvars),
            |b, &(n, tvs)| {
                let mut tx = init_transaction();
                b.iter(|| {
                    for i in 0..n {
                        #[allow(unused_must_use)]
                        black_box(tx.write(&tvs[i as usize], new_value));
                    }
                })
            },
        );
    }
    g2.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
