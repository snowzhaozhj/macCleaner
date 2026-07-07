//! 磁盘分析增量树构建器：把流式 `AnalyzeEvent::Entry` 集成成 `DirNode` 树。
//!
//! 自成一体、不依赖 TUI 状态，从 `lib.rs` 抽出以收敛主文件体积。
//!
//! ## 为什么是路径键控插入（issue #20 / plan 010 U4）
//! 旧实现依赖 jwalk 的**严格 DFS 序**：靠 `depth` 相对上一个 entry 的增减在深度栈上导航。
//! park 引擎按**完成序**批交付（非 DFS 序），DFS 假设不再成立。改为**路径键控插入**：
//! 用每个 entry 的**父路径**在 `locator`（`路径 → 到达该节点的 children 索引链`）里 O(1)
//! 查父后插入，与交付顺序**完全解耦**。
//!
//! `park_walk` 已保证「父目录 entry 先于其任何子目录的批到达消费端」（见其先发批后入队），
//! 故正常情况下父必先在。但为兜底并发交付的任何乱序（以及未来后端变化），
//! 额外用 `orphans` 缓存「父尚未到达」的 entry，父到达时回填——正确性不依赖到达顺序。

use crate::models::DirNode;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 尚未能插入（父路径未到达）的挂起 entry。
struct PendingEntry {
    name: String,
    path: PathBuf,
    size: u64,
    is_file: bool,
}

#[derive(Default)]
pub struct IncrementalTreeBuilder {
    /// `路径 → 从 root 到达该节点的 children 索引链`。root 映射到空链 `[]`。
    /// 仅记录**目录**节点（文件不会成为别人的父）。
    locator: HashMap<PathBuf, Vec<usize>>,
    /// 父路径尚未到达的孤儿 entry，按父路径缓存；父到达后回填。
    orphans: HashMap<PathBuf, Vec<PendingEntry>>,
    /// 是否已登记 root 路径（首个 entry 到达时以 `tree_root.path` 懒登记）。
    root_registered: bool,
}

impl IncrementalTreeBuilder {
    pub fn new() -> Self {
        Self {
            locator: HashMap::new(),
            orphans: HashMap::new(),
            root_registered: false,
        }
    }

    /// 将一个 entry 集成到 `tree_root`。交付顺序无关：父未到达则缓存为孤儿，父到达时回填。
    /// 返回 `Option` 仅为兼容旧签名；异常（无父路径）时静默跳过，不 panic。
    pub fn integrate_entry(
        &mut self,
        tree_root: &mut DirNode,
        name: String,
        path: PathBuf,
        size: u64,
        is_file: bool,
    ) -> Option<()> {
        if !self.root_registered {
            self.locator.insert(tree_root.path.clone(), Vec::new());
            self.root_registered = true;
        }
        self.insert_or_stash(tree_root, PendingEntry { name, path, size, is_file });
        Some(())
    }

    /// 用显式工作队列插入 entry 及其（因它到达而解锁的）所有孤儿后代，避免深递归爆栈。
    fn insert_or_stash(&mut self, tree_root: &mut DirNode, entry: PendingEntry) {
        let mut queue = vec![entry];
        while let Some(e) = queue.pop() {
            let Some(parent_path) = e.path.parent().map(Path::to_path_buf) else {
                continue; // 无父路径（根级 `/`）——不该出现在分析树里，跳过
            };
            let Some(parent_chain) = self.locator.get(&parent_path).cloned() else {
                // 父未到达：缓存为孤儿，等父到达时回填。
                self.orphans.entry(parent_path).or_default().push(e);
                continue;
            };
            let Some(parent) = Self::navigate(tree_root, &parent_chain) else {
                continue; // locator 与树不一致（不应发生）——跳过不 panic
            };
            let new_idx = parent.children.len();
            if e.is_file {
                parent.children.push(DirNode::new_file(e.path.clone(), e.name, e.size));
                if e.size > 0 {
                    // 文件大小向上传播到父及所有祖先（含 root）。
                    Self::propagate(tree_root, &parent_chain, e.size);
                }
            } else {
                parent.children.push(DirNode::new_dir(e.path.clone(), e.name));
                // 登记新目录的索引链；回填等待它的孤儿（级联解锁整个子树）。
                let mut chain = parent_chain.clone();
                chain.push(new_idx);
                self.locator.insert(e.path.clone(), chain);
                if let Some(waiting) = self.orphans.remove(&e.path) {
                    queue.extend(waiting);
                }
            }
        }
    }

    /// 沿索引链导航到目标节点（空链 → root）。
    fn navigate<'a>(tree_root: &'a mut DirNode, chain: &[usize]) -> Option<&'a mut DirNode> {
        let mut node = tree_root;
        for &idx in chain {
            node = node.children.get_mut(idx)?;
        }
        Some(node)
    }

    /// 把 `size` 加到 root 及 `parent_chain` 上的每个节点（= 新文件的父及所有祖先）。
    fn propagate(tree_root: &mut DirNode, parent_chain: &[usize], size: u64) {
        tree_root.size += size;
        let mut node = tree_root;
        for &idx in parent_chain {
            node = match node.children.get_mut(idx) {
                Some(n) => n,
                None => return, // 链不一致，停止传播但不 panic
            };
            node.size += size;
        }
    }

    /// 遍历完成后递归排序所有 children（按 size 降序）。
    pub fn finalize(tree_root: &mut DirNode) {
        fn sort_recursive(node: &mut DirNode) {
            node.children.sort_by_key(|c| std::cmp::Reverse(c.size));
            for child in &mut node.children {
                if !child.is_file {
                    sort_recursive(child);
                }
            }
        }
        sort_recursive(tree_root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(path: &str) -> DirNode {
        DirNode::new_dir(PathBuf::from(path), "root".into())
    }

    /// 按给定顺序把 (path, size, `is_file`) 喂给 builder，返回建好的树。
    fn build(root_path: &str, entries: &[(&str, u64, bool)]) -> DirNode {
        let mut tree = root(root_path);
        let mut b = IncrementalTreeBuilder::new();
        for (p, size, is_file) in entries {
            let path = PathBuf::from(p);
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            b.integrate_entry(&mut tree, name, path, *size, *is_file).unwrap();
        }
        tree
    }

    /// 递归找某路径的节点。
    fn find<'a>(node: &'a DirNode, path: &str) -> Option<&'a DirNode> {
        if node.path.as_path() == Path::new(path) {
            return Some(node);
        }
        node.children.iter().find_map(|c| find(c, path))
    }

    const TREE: &[(&str, u64, bool)] = &[
        ("/r/a", 0, false),
        ("/r/a/f1", 100, true),
        ("/r/a/b", 0, false),
        ("/r/a/b/f2", 50, true),
        ("/r/c", 0, false),
        ("/r/c/f3", 200, true),
    ];

    #[test]
    fn in_order_builds_correct_tree_and_sizes() {
        let tree = build("/r", TREE);
        assert_eq!(tree.size, 350, "root size = 100+50+200");
        assert_eq!(find(&tree, "/r/a").unwrap().size, 150, "a = f1(100)+b/f2(50)");
        assert_eq!(find(&tree, "/r/a/b").unwrap().size, 50);
        assert_eq!(find(&tree, "/r/c").unwrap().size, 200);
        assert_eq!(find(&tree, "/r/a/f1").unwrap().size, 100);
    }

    #[test]
    fn out_of_order_delivery_builds_identical_tree() {
        // 覆盖 R3：模拟 park 并发批交付把子目录批穿插到父之前——孤儿缓存回填后结果一致。
        // 逆序（叶子先、父后）是最坏乱序。
        let mut reversed: Vec<(&str, u64, bool)> = TREE.to_vec();
        reversed.reverse();
        let tree = build("/r", &reversed);
        assert_eq!(tree.size, 350, "逆序交付 root size 仍应为 350");
        assert_eq!(find(&tree, "/r/a").unwrap().size, 150, "逆序下 a 子树 size 仍正确");
        assert_eq!(find(&tree, "/r/a/b").unwrap().size, 50);
        assert_eq!(find(&tree, "/r/c").unwrap().size, 200);
        // 结构等价：a 有 2 个 children（f1, b），c 有 1 个（f3）。
        assert_eq!(find(&tree, "/r/a").unwrap().children.len(), 2);
        assert_eq!(find(&tree, "/r/c").unwrap().children.len(), 1);
    }

    #[test]
    fn interleaved_sibling_subtrees() {
        // 子目录 a、c 的批交错到达（c 的后代夹在 a 的后代之间）。
        let interleaved: &[(&str, u64, bool)] = &[
            ("/r/a", 0, false),
            ("/r/c", 0, false),
            ("/r/c/f3", 200, true),
            ("/r/a/f1", 100, true),
            ("/r/a/b", 0, false),
            ("/r/a/b/f2", 50, true),
        ];
        let tree = build("/r", interleaved);
        assert_eq!(tree.size, 350);
        assert_eq!(find(&tree, "/r/a").unwrap().size, 150);
        assert_eq!(find(&tree, "/r/c").unwrap().size, 200);
    }

    #[test]
    fn finalize_sorts_children_by_size_desc() {
        // 覆盖 R5：finalize 后各层 children 按 size 降序（跟随最大项依赖显示序 0 = 最大）。
        let mut tree = build("/r", TREE);
        IncrementalTreeBuilder::finalize(&mut tree);
        // root 的 children：a(150) 应在 c(200) 之后 → c 先。
        assert_eq!(tree.children[0].path, PathBuf::from("/r/c"), "最大子项 c 应排首");
        assert_eq!(tree.children[1].path, PathBuf::from("/r/a"));
        // a 的 children：f1(100) 应在 b(50) 之前。
        let a = find(&tree, "/r/a").unwrap();
        assert_eq!(a.children[0].path, PathBuf::from("/r/a/f1"));
    }

    #[test]
    fn equal_size_sort_is_stable() {
        // 等值 size 稳定序不抖动（sort_by_key 稳定）。
        let entries: &[(&str, u64, bool)] = &[
            ("/r/x", 0, false),
            ("/r/x/a", 10, true),
            ("/r/x/b", 10, true),
            ("/r/x/c", 10, true),
        ];
        let mut tree = build("/r", entries);
        IncrementalTreeBuilder::finalize(&mut tree);
        let x = find(&tree, "/r/x").unwrap();
        let order: Vec<_> = x.children.iter().map(|c| c.name.clone()).collect();
        assert_eq!(order, vec!["a", "b", "c"], "等值 size 保持插入序");
    }

    #[test]
    fn deeply_nested_chain() {
        // 深链不爆栈、size 一路传到 root。
        let mut entries: Vec<(String, u64, bool)> = Vec::new();
        let mut p = String::from("/r");
        for i in 0..500 {
            p = format!("{p}/d{i}");
            entries.push((p.clone(), 0, false));
        }
        p = format!("{p}/leaf");
        entries.push((p.clone(), 7, true));
        let refs: Vec<(&str, u64, bool)> =
            entries.iter().map(|(s, sz, f)| (s.as_str(), *sz, *f)).collect();
        let tree = build("/r", &refs);
        assert_eq!(tree.size, 7, "深链叶子 size 应传到 root");
        assert_eq!(find(&tree, "/r/d0").unwrap().size, 7);
    }
}
