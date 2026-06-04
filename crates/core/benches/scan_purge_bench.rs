use criterion::{criterion_group, criterion_main, Criterion};
use mc_core::engine::Engine;
use mc_core::progress::{ProgressEvent, ProgressReporter};
use std::path::PathBuf;

struct NullReporter;

impl ProgressReporter for NullReporter {
    fn on_event(&self, _event: ProgressEvent) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

fn bench_scan_purge(c: &mut Criterion) {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));

    let mut group = c.benchmark_group("scan_purge");
    group.sample_size(10);
    group.measurement_time(std::time::Duration::from_mins(1));

    group.bench_function("home_dir", |b| {
        b.iter(|| {
            let reporter = NullReporter;
            let _ = Engine::scan_purge(&home, &reporter);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_scan_purge);
criterion_main!(benches);
