use criterion::{
    criterion_group, criterion_main, AxisScale, BenchmarkId, Criterion, PlotConfiguration,
    Throughput,
};
use fast_stm::{commit_transaction, init_transaction, TVar};

/// Transaction commit routine benchmarks
pub fn criterion_benchmark(c: &mut Criterion) {
    // G1

    let n_accs = [1_000, 10_000, 100_000, 1_000_000];
    let tvars: Vec<_> = (0..*n_accs.last().unwrap())
        .map(|_| TVar::new(42_u32))
        .collect();

    let mut g1 = c.benchmark_group("n-reads-commit-times");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    g1.plot_config(plot_config);
    for n_read in n_accs {
        g1.throughput(Throughput::Elements(n_read));
        g1.bench_with_input(
            BenchmarkId::new("TVar::<u32>::read", n_read),
            &(n_read, &tvars),
            |b, &(n, tvs)| {
                let mut tx = init_transaction();
                for i in 0..n {
                    let _ = tx.read(&tvs[i as usize]);
                }
                b.iter(|| {
                    commit_transaction(&mut tx);
                });
            },
        );
    }
    g1.finish();

    let mut g2 = c.benchmark_group("n-writes-commit-times");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    g2.plot_config(plot_config);
    for n_write in n_accs {
        g2.throughput(Throughput::Elements(n_write));
        g2.bench_with_input(
            BenchmarkId::new("TVar::<u32>::write", n_write),
            &(n_write, &tvars),
            |b, &(n, tvs)| {
                let mut tx = init_transaction();
                for i in 0..n {
                    let _ = tx.write(&tvs[i as usize], 12345);
                }
                b.iter(|| {
                    commit_transaction(&mut tx);
                });
            },
        );
    }
    g2.finish();

    let mut g3 = c.benchmark_group("n-accesses-commit-times");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    g3.plot_config(plot_config);
    for n_accesses in n_accs {
        g3.throughput(Throughput::Elements(n_accesses));
        g3.bench_with_input(
            BenchmarkId::new("mixed reads and writes", n_accesses),
            &(n_accesses, &tvars),
            |b, &(n, tvs)| {
                let mut tx = init_transaction();
                for i in 0..n / 2 {
                    let _ = tx.read(&tvs[i as usize]);
                }
                for i in n / 2..n {
                    let _ = tx.write(&tvs[i as usize], 12345);
                }
                b.iter(|| {
                    commit_transaction(&mut tx);
                });
            },
        );
    }
    g3.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
