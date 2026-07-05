//! park 式（阻塞/挂起）并行目录遍历器——替代 jwalk 0.8.1 的自旋式并行消费端。
//!
//! ## 为什么存在（见 issue #20 / plan 010）
//! jwalk 并行 walk 的消费端（`OrderedQueueIter::next` 拉 in-order 结果）用**无条件
//! `thread::yield_now()` 忙等**，且**与池线程数无关**（walk 池 2/3/4/10 线程 CPU 全 ~130%，
//! 只有 Serial ~31%）。空闲线程不 park 而自旋，白烧 ~1 核。本模块的 worker **空闲即阻塞在
//! channel recv（park，0 CPU）**，把那 25% 自旋样本变成 0-CPU 挂起等待。
//!
//! ## 语义与 jwalk（`scanner::create_walker`）严格对齐
//! - **不跟随符号链接**：`file_type()` 来自 `d_type`，符号链接 `is_dir()==false` 不深入；
//!   文件大小走 `symlink_metadata`（= lstat，不跟随），与 `prefetch_metadata` 的
//!   `DirEntry::metadata`（同为 lstat）逐字节一致。
//! - **不排序**：目录内 entry 顺序 = readdir 顺序（jwalk 默认亦不排序）；三个消费端
//!   （clean 归类 / purge 剪枝收集 / analyze 键控插入）均与交付顺序无关。
//! - **根目录本身**作为一个 entry 交付（jwalk 把根作为顶层 entry 上报），口径一致。
//!
//! ## 终止（pending 计数）
//! 根入队时 `pending=1`；worker 对**每个子目录先 `fetch_add` 再入队**、处理完自身后
//! `fetch_sub`；减到 0 的那个 worker 广播 N 个 `Done` 哨兵解散全体。无需 join 顺序。
//!
//! ## 取消（协作式）
//! 主线程消费时查 `is_cancelled()`，一旦为真置内部 `cancelled` flag：worker 每目录查一次，
//! 置位后跳过读目录、仅 `fetch_sub` 快速排空 pending 后退出。对齐 `scanner` 的
//! `reporter.is_cancelled()` 约定。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossbeam_channel::unbounded;

/// 交付给消费端的一个 entry（目录或文件）。
pub struct WalkEntry {
    pub path: PathBuf,
    pub is_dir: bool,
    /// 文件大小（字节）；目录恒为 0。仅当 `on_read_dir` 调用了 [`prefetch_sizes`]（或自行
    /// 填充）时对文件有意义——purge 剪枝遍历不需要文件大小，故不预取，此值保持 0。
    pub size: u64,
}

/// `on_read_dir` 回调可就地改写的目录 child：`retain` 剪枝 + 可选填 `size`。
pub struct DirChild {
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

impl DirChild {
    /// child 的文件名（末段），供 purge 规则按目录名匹配。
    pub fn file_name(&self) -> std::borrow::Cow<'_, str> {
        self.path
            .file_name()
            .map_or(std::borrow::Cow::Borrowed(""), |n| n.to_string_lossy())
    }
}

/// 对每个**非目录** child 做 lstat 填 `size`（与 `scanner::prefetch_metadata` 同口径：
/// `symlink_metadata` = lstat，不跟随符号链接）。目录 child 不动（大小无意义）。
pub fn prefetch_sizes(children: &mut [DirChild]) {
    for c in children.iter_mut() {
        if !c.is_dir {
            c.size = std::fs::symlink_metadata(&c.path).map_or(0, |m| m.len());
        }
    }
}

enum Job {
    Dir(PathBuf),
    Done,
}

/// 阻塞式 work-queue 并行遍历。
///
/// - `on_read_dir`：每读完一个目录的 children 后调用一次（在 worker 线程上），用于
///   `retain` 剪枝 + 可选 [`prefetch_sizes`]。**剪掉的目录不会被深入**。需 `Sync`
///   （多 worker 并发调用）。
/// - `is_cancelled`：主线程每收到一批 entry 查一次；返回 true 即尽快中止遍历。
/// - `consume`：主线程按**完成序**逐个收到 [`WalkEntry`]（含根、所有子目录、所有文件）。
///   在单一线程上调用，故其捕获状态无需 `Send`。
///
/// `threads` 会被夹到 `>=1`。整个遍历在 `thread::scope` 内完成，返回即全部 worker 已 join。
pub fn park_walk<F, X, C>(
    root: &Path,
    threads: usize,
    on_read_dir: F,
    is_cancelled: X,
    mut consume: C,
) where
    F: Fn(&mut Vec<DirChild>) + Sync,
    X: Fn() -> bool,
    C: FnMut(WalkEntry),
{
    let threads = threads.max(1);
    let (job_tx, job_rx) = unbounded::<Job>();
    let (res_tx, res_rx) = unbounded::<Vec<WalkEntry>>();
    let pending = AtomicUsize::new(1);
    let cancelled = AtomicBool::new(false);

    // 根目录本身作为一个 entry（对齐 jwalk 把根作为顶层 entry 上报）
    consume(WalkEntry { path: root.to_path_buf(), is_dir: true, size: 0 });
    job_tx.send(Job::Dir(root.to_path_buf())).expect("job channel just created");

    std::thread::scope(|scope| {
        for _ in 0..threads {
            let job_rx = job_rx.clone();
            let job_tx = job_tx.clone();
            let res_tx = res_tx.clone();
            let on_read_dir = &on_read_dir;
            let pending = &pending;
            let cancelled = &cancelled;
            scope.spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    let Job::Dir(dir) = job else { break };
                    if !cancelled.load(Ordering::Relaxed) {
                        let (batch, subdirs) = read_one_dir(&dir, on_read_dir);
                        // **先发批、后入队子目录**：保证父目录（作为 entry 在本批内）先于其
                        // 任何子目录的批到达消费端。否则多 worker 下，子目录可能被另一 worker
                        // 抢先读完并发出其批，早于本批——analyze 键控插入会找不到父（issue #20
                        // KTD3 的顺序假设在并发批交付下需此保证兜底）。消费端可能已因取消退出，
                        // unbounded 发送不阻塞、失败忽略。
                        let _ = res_tx.send(batch);
                        for subdir in subdirs {
                            // 先计数后入队：pending 不会在还有活时误降到 0。
                            pending.fetch_add(1, Ordering::AcqRel);
                            let _ = job_tx.send(Job::Dir(subdir));
                        }
                    }
                    // 自身处理完毕；若是最后一个待处理目录，广播 N 个哨兵解散全体。
                    if pending.fetch_sub(1, Ordering::AcqRel) == 1 {
                        for _ in 0..threads {
                            let _ = job_tx.send(Job::Done);
                        }
                    }
                }
            });
        }
        // 主线程放弃自己持有的发送/接收克隆：res_rx 在全部 worker 退出后自然断开。
        drop(res_tx);
        drop(job_tx);
        drop(job_rx);

        // 消费端：阻塞 recv（挂起等待，不忙等），流式逐 entry 交付。
        for batch in res_rx {
            for entry in batch {
                consume(entry);
            }
            if is_cancelled() {
                cancelled.store(true, Ordering::Release);
            }
        }
    });
}

/// 读取单个目录：`on_read_dir` 剪枝/预取后，返回 `(该目录全部保留 child 攒成的一批, 需入队的子目录)`。
/// **不**在此入队/改 pending——由 worker 在**发批之后**再入队，以保证父先于子到达消费端。
fn read_one_dir<F>(dir: &Path, on_read_dir: &F) -> (Vec<WalkEntry>, Vec<PathBuf>)
where
    F: Fn(&mut Vec<DirChild>),
{
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return (Vec::new(), Vec::new()); // 与 jwalk 一致：读不到的目录跳过
    };
    let mut children: Vec<DirChild> = Vec::new();
    for entry in read_dir.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        children.push(DirChild {
            path: entry.path(),
            is_dir: file_type.is_dir(),
            size: 0,
        });
    }

    on_read_dir(&mut children); // retain 剪枝 + 可选 prefetch

    let mut batch = Vec::with_capacity(children.len());
    let mut subdirs = Vec::new();
    for child in children {
        if child.is_dir {
            subdirs.push(child.path.clone());
        }
        batch.push(WalkEntry { path: child.path, is_dir: child.is_dir, size: child.size });
    }
    (batch, subdirs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    /// 参照实现：串行 jwalk 遍历，返回 (files, dirs, bytes)——作为逐字节对账基线。
    fn jwalk_reference(root: &Path) -> (u64, u64, u64) {
        let walker = crate::MetaWalkDir::new(root)
            .skip_hidden(false)
            .follow_links(false)
            .parallelism(jwalk::Parallelism::Serial)
            .process_read_dir(|_d, _p, _s, children| crate::prefetch_metadata(children));
        let (mut files, mut dirs, mut bytes) = (0u64, 0u64, 0u64);
        for entry in walker.into_iter().flatten() {
            if entry.file_type().is_dir() {
                dirs += 1;
            } else {
                files += 1;
                bytes += entry.client_state.unwrap_or(0);
            }
        }
        (files, dirs, bytes)
    }

    /// park 全树遍历（预取大小），返回 (files, dirs, bytes)。
    fn park_totals(root: &Path, threads: usize) -> (u64, u64, u64) {
        let (mut files, mut dirs, mut bytes) = (0u64, 0u64, 0u64);
        park_walk(
            root,
            threads,
            |children| prefetch_sizes(children),
            || false,
            |e| {
                if e.is_dir {
                    dirs += 1;
                } else {
                    files += 1;
                    bytes += e.size;
                }
            },
        );
        (files, dirs, bytes)
    }

    /// 造一棵可复算的合成树：top×mid 两级 + 每目录若干文件。
    fn mktree(root: &Path, top: usize, mid: usize, files_per_dir: usize) -> u64 {
        let mut total_bytes = 0u64;
        let fill = |dir: &Path, acc: &mut u64| {
            for f in 0..files_per_dir {
                let len = 64 + (f * 128) % 1025;
                std::fs::write(dir.join(format!("f{f}.bin")), vec![b'x'; len]).unwrap();
                *acc += len as u64;
            }
        };
        for t in 0..top {
            let td = root.join(format!("t{t}"));
            std::fs::create_dir_all(&td).unwrap();
            fill(&td, &mut total_bytes);
            for m in 0..mid {
                let md = td.join(format!("m{m}"));
                std::fs::create_dir_all(&md).unwrap();
                fill(&md, &mut total_bytes);
            }
        }
        total_bytes
    }

    #[test]
    fn park_matches_jwalk_byte_for_byte() {
        let tmp = tempdir().unwrap();
        let expected_bytes = mktree(tmp.path(), 4, 5, 7);
        let reference = jwalk_reference(tmp.path());
        // 多线程数下都应与 jwalk 逐字节一致（完成序不影响聚合）。
        for threads in [1, 2, 8] {
            let got = park_totals(tmp.path(), threads);
            assert_eq!(got, reference, "threads={threads} 应与 jwalk 参照一致");
            assert_eq!(got.2, expected_bytes, "threads={threads} bytes 应等于造树累计");
        }
    }

    #[test]
    fn park_empty_dir() {
        let tmp = tempdir().unwrap();
        // 空目录：仅根一个 dir entry，0 文件 0 字节。
        assert_eq!(park_totals(tmp.path(), 3), (0, 1, 0));
        assert_eq!(park_totals(tmp.path(), 3), jwalk_reference(tmp.path()));
    }

    #[test]
    fn park_single_file_root() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("only.bin"), vec![b'z'; 42]).unwrap();
        assert_eq!(park_totals(tmp.path(), 3), (1, 1, 42));
        assert_eq!(park_totals(tmp.path(), 3), jwalk_reference(tmp.path()));
    }

    #[test]
    fn park_nonexistent_path() {
        // 不存在的根：read_dir 失败，仅根 entry 本身（jwalk 亦把根作为 entry）。
        let got = park_totals(Path::new("/nonexistent_park_xyz_123"), 3);
        assert_eq!(got, (0, 1, 0));
    }

    #[test]
    fn park_does_not_follow_symlinks() {
        // 覆盖 R3：符号链接不跟随——软链指向的外部大文件不计入。
        let tmp = tempdir().unwrap();
        let real = tmp.path().join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("big.bin"), vec![0u8; 10_000]).unwrap();

        let proj = tmp.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("small.txt"), vec![b'x'; 3]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, proj.join("link_to_real")).unwrap();

        // 只遍历 proj：不得跟随 link_to_real 进入 real（10_000 字节不计入）。
        let got = park_totals(&proj, 3);
        assert!(got.2 < 10_000, "不应跟随符号链接，实际 bytes={}", got.2);
        // 与 jwalk 参照逐字节一致（含符号链接自身 lstat 大小口径）。
        assert_eq!(got, jwalk_reference(&proj));
    }

    #[test]
    fn park_pruning_via_on_read_dir() {
        // on_read_dir 剪掉的目录不深入：模拟 SKIP_DIRS 语义。
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("keep.txt"), vec![b'a'; 5]).unwrap();
        let skip = tmp.path().join("skipme");
        std::fs::create_dir_all(&skip).unwrap();
        std::fs::write(skip.join("hidden.bin"), vec![b'b'; 9999]).unwrap();

        let (mut files, mut bytes) = (0u64, 0u64);
        park_walk(
            tmp.path(),
            3,
            |children| {
                children.retain(|c| !(c.is_dir && c.file_name() == "skipme"));
                prefetch_sizes(children);
            },
            || false,
            |e| {
                if !e.is_dir {
                    files += 1;
                    bytes += e.size;
                }
            },
        );
        assert_eq!((files, bytes), (1, 5), "skipme 子树应被剪枝，只剩 keep.txt");
    }

    #[test]
    fn park_deep_unbalanced_tree_terminates() {
        // 深度不平衡：一条极深链 + 众多浅目录。验证 pending 计数正确归零、Done 广播终止、无死锁。
        let tmp = tempdir().unwrap();
        // 深链：200 层
        let mut deep = tmp.path().to_path_buf();
        for i in 0..200 {
            deep = deep.join(format!("d{i}"));
            std::fs::create_dir_all(&deep).unwrap();
            std::fs::write(deep.join("x.bin"), vec![b'q'; 10]).unwrap();
        }
        // 浅目录：150 个
        for i in 0..150 {
            let s = tmp.path().join(format!("s{i}"));
            std::fs::create_dir_all(&s).unwrap();
            std::fs::write(s.join("y.bin"), vec![b'w'; 20]).unwrap();
        }
        // 单 worker 与多 worker 都必须终止且结果一致。
        let r1 = park_totals(tmp.path(), 1);
        let r8 = park_totals(tmp.path(), 8);
        assert_eq!(r1, r8, "worker 数不应改变结果");
        assert_eq!(r1, jwalk_reference(tmp.path()), "深不平衡树应与 jwalk 一致");
    }

    #[test]
    fn park_cancellation_stops_early() {
        // 覆盖 R4：置取消后遍历及时停止，已发现项 < 全量且无 panic。
        let tmp = tempdir().unwrap();
        mktree(tmp.path(), 6, 6, 20); // 足够大
        let full = jwalk_reference(tmp.path());

        let seen = Mutex::new(0u64);
        // 收到第一批后即取消：worker 应跳过后续目录，最终发现项远小于全量。
        park_walk(
            tmp.path(),
            3,
            |children| prefetch_sizes(children),
            || *seen.lock().unwrap() > 0, // 一旦消费过任何 entry 即请求取消
            |_e| {
                *seen.lock().unwrap() += 1;
            },
        );
        let discovered = *seen.lock().unwrap();
        let total = full.0 + full.1;
        assert!(discovered > 0, "至少发现根 entry");
        assert!(
            discovered < total,
            "取消应提前终止：discovered={discovered} 应 < 全量 {total}"
        );
    }

    #[test]
    fn park_on_read_dir_called_concurrently_is_sync() {
        // on_read_dir 在多 worker 上并发调用（Sync）：用共享计数器验证被调到、无数据竞争。
        let tmp = tempdir().unwrap();
        mktree(tmp.path(), 3, 3, 2);
        let dir_reads = Mutex::new(0u64);
        park_walk(
            tmp.path(),
            4,
            |children| {
                *dir_reads.lock().unwrap() += 1;
                prefetch_sizes(children);
            },
            || false,
            |_e| {},
        );
        // 目录数 = 1(root 不经 on_read_dir，但其 children 读一次) ... on_read_dir 每个被读目录调一次。
        // 造树 dirs = top + top*mid = 3 + 9 = 12，加根 = 13 个目录被 read_dir。
        assert_eq!(*dir_reads.lock().unwrap(), 13, "每个目录 read 一次 on_read_dir");
    }
}
