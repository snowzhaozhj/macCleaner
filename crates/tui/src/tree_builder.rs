//! 磁盘分析增量树构建器：把 jwalk 的 DFS 序 `AnalyzeEvent::Entry` 流式集成成 `DirNode` 树。
//!
//! 自成一体、不依赖 TUI 状态，从 `lib.rs` 抽出以收敛主文件体积。

use mc_core::models::DirNode;
use std::path::PathBuf;

pub(crate) struct IncrementalTreeBuilder {
    /// `深度栈：depth_stack`[d] = 深度 d 的当前节点在其父 children 中的索引
    depth_stack: Vec<usize>,
    previous_depth: usize,
}

impl IncrementalTreeBuilder {
    pub(crate) fn new() -> Self {
        Self {
            depth_stack: Vec::new(),
            previous_depth: 0,
        }
    }

    /// 将一个 `AnalyzeEvent::Entry` 集成到 `tree_root`。
    /// jwalk 保证 DFS 序，depth 相对于 `previous_depth` 的关系决定导航方向。
    /// 返回 Option：异常 depth 时返回 None 并跳过，不 panic。
    pub(crate) fn integrate_entry(
        &mut self,
        tree_root: &mut DirNode,
        depth: usize,
        name: String,
        path: PathBuf,
        size: u64,
        is_file: bool,
    ) -> Option<()> {
        // 运行时安全检查
        if depth == 0 || depth > self.previous_depth + 1 {
            return None; // 跳过异常 entry，不 panic
        }

        // 深度导航
        if depth > self.previous_depth {
            if self.previous_depth > 0 {
                // 进入子目录：push 当前深度节点的最后一个 children 索引
                let parent = Self::navigate_to_parent(tree_root, &self.depth_stack, self.previous_depth)?;
                if parent.children.is_empty() {
                    return None;
                }
                self.depth_stack.push(parent.children.len() - 1);
            }
            // previous_depth == 0 时是第一个 entry，直接添加到 tree_root，无需 push
        } else if depth < self.previous_depth {
            // 回退到上层目录
            self.depth_stack.truncate(depth.saturating_sub(1));
        }
        // depth == previous_depth: 不变

        let parent = Self::navigate_to_parent(tree_root, &self.depth_stack, depth)?;
        let new_idx = parent.children.len();
        if is_file {
            parent.children.push(DirNode::new_file(path, name, size));
        } else {
            parent.children.push(DirNode::new_dir(path, name));
        }

        // 更新 depth_stack 以指向新节点
        if self.depth_stack.len() < depth {
            self.depth_stack.push(new_idx);
        } else if let Some(slot) = self.depth_stack.get_mut(depth - 1) {
            *slot = new_idx;
        }

        if is_file && size > 0 {
            Self::propagate_size(tree_root, &self.depth_stack, depth, size);
        }

        self.previous_depth = depth;
        Some(())
    }

    /// 导航到目标深度的父节点，返回 Option 而非裸索引
    fn navigate_to_parent<'a>(
        tree_root: &'a mut DirNode,
        depth_stack: &[usize],
        target_depth: usize,
    ) -> Option<&'a mut DirNode> {
        let mut node = tree_root;
        for i in 0..target_depth.saturating_sub(1) {
            let idx = *depth_stack.get(i)?;
            node = node.children.get_mut(idx)?;
        }
        Some(node)
    }

    /// 向上传播 size 到所有祖先节点
    fn propagate_size(
        tree_root: &mut DirNode,
        depth_stack: &[usize],
        depth: usize,
        size: u64,
    ) {
        tree_root.size += size;
        let mut node = tree_root;
        for i in 0..depth.saturating_sub(1) {
            let idx = match depth_stack.get(i) {
                Some(&idx) => idx,
                None => return, // 栈不一致，停止传播但不 panic
            };
            node = match node.children.get_mut(idx) {
                Some(n) => n,
                None => return,
            };
            node.size += size;
        }
    }

    /// 遍历完成后递归排序所有 children（按 size 降序）
    pub(crate) fn finalize(tree_root: &mut DirNode) {
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
