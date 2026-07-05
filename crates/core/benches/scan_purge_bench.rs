use criterion::{criterion_group, criterion_main, Criterion};
use mc_core::engine::Engine;
use mc_core::progress::{ProgressEvent, ProgressReporter};
use std::path::Path;

struct NullReporter;

impl ProgressReporter for NullReporter {
    fn on_event(&self, _event: ProgressEvent) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// 构建可复现的合成目录树（plan 009 U1/KTD1）：替代原来的真实 home——真实 home 内容随机器
/// 漂移、不可跨机/跨配置复现，无法做稳定的"线程数↔速度"对照。
///
/// 结构：`projects` 个含 `package.json` 守卫 + `node_modules` 的项目；每个 `node_modules` 铺
/// `files_per` 个文件，分散到 3 层嵌套子目录，让 `dir_size` 有真实的递归遍历量。
/// 项目数 > `dir_size` 池线程数，才能体现并行/嵌套池差异。
fn build_synthetic_tree(base: &Path, projects: usize, files_per: usize) {
    for p in 0..projects {
        let proj = base.join(format!("proj_{p}"));
        let nm = proj.join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(proj.join("package.json"), "{}").unwrap();
        for f in 0..files_per {
            // 3 层嵌套：pkg_{a}/lib_{b}/file_{f}
            let a = f % 8;
            let b = (f / 8) % 8;
            let dir = nm.join(format!("pkg_{a}")).join(format!("lib_{b}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("file_{f}.js")), b"module.exports={}").unwrap();
        }
    }
}

fn bench_scan_purge(c: &mut Criterion) {
    // 合成树建一次，全程复用（criterion 多次 iter 都扫同一棵树）。
    // 规模：24 项目 × 300 文件 ≈ 7200 文件，足以让 dir_size 成为主耗时且体现并发差异。
    let tmp = tempfile::tempdir().expect("create temp dir");
    build_synthetic_tree(tmp.path(), 24, 300);
    let base = tmp.path().to_path_buf();

    let mut group = c.benchmark_group("scan_purge");
    group.sample_size(20);
    group.measurement_time(std::time::Duration::from_secs(20));

    // 并发通过 MC_WALK_THREADS / MC_DIRSIZE_THREADS 环境变量外部注入，跑多轮对照：
    //   MC_DIRSIZE_THREADS=1 cargo bench -p mc-core -- synthetic_tree
    //   MC_DIRSIZE_THREADS=4 cargo bench -p mc-core -- synthetic_tree
    group.bench_function("synthetic_tree", |b| {
        b.iter(|| {
            let reporter = NullReporter;
            let _ = Engine::scan_purge(&base, &reporter);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_scan_purge);
criterion_main!(benches);
