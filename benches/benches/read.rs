use std::hint::black_box;
use std::sync::atomic::{AtomicU32, Ordering};

use criterion::{
    criterion_group, criterion_main, AxisScale, BenchmarkId, Criterion, PlotConfiguration,
    Throughput,
};
use fast_stm::{atomically, init_transaction, TVar};

/// Read routines benchmarks
///
/// # Group 1
///
/// Compare:
///
/// - `TVar::read` in transaction
/// - `TVar::read` alone
/// - `TVar::read_atomic`
/// - `Atomic::load`
///
/// execution times.
///
/// # Group 2
///
/// Execution times for N reads of different variables.
///
/// # Group 3
///
/// Execution time of the N-th read of a transaction.
pub fn criterion_benchmark(c: &mut Criterion) {
    // G1
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

    // G2
    let n_reads = [1_000, 10_000, 100_000, 1_000_000];
    let tvars: Vec<_> = (0..*n_reads.last().unwrap())
        .map(|_| TVar::new(42_u32))
        .collect();

    let mut g2 = c.benchmark_group("read-times-vs-n-reads");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    g2.plot_config(plot_config);

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

    // G3

    let mut g3 = c.benchmark_group("nth-read-times");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    g3.plot_config(plot_config);

    for n_read in n_reads {
        g3.bench_with_input(
            BenchmarkId::new("TVar::<u32>::read", n_read),
            &(n_read, &tvars),
            |b, &(n, tvs)| {
                let mut tx = init_transaction();
                // read n-1 variables, then benchmark the next read
                for i in 0..n - 1 {
                    let _ = tx.read(&tvs[i as usize]);
                }
                b.iter(|| tx.read(&tvs[n as usize - 1]))
            },
        );
    }
    g3.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
