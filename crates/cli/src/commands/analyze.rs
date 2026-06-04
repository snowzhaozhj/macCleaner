use crate::{Cli, Commands};
use mc_core::models::DirNode;
use mc_core::platform;

use anyhow::Result;
use humansize::{format_size, DECIMAL};
use std::path::{Path, PathBuf};

pub fn run(cli: &Cli) -> Result<()> {
    let (path, threshold_mb) = match &cli.command {
        Some(Commands::Analyze { path, threshold }) => {
            let p = path
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(platform::get_home_dir);
            (p, *threshold)
        }
        _ => (platform::get_home_dir(), 100),
    };

    let threshold_bytes = threshold_mb * 1024 * 1024;

    if !path.exists() {
        anyhow::bail!("路径不存在: {}", path.display());
    }

    eprintln!("正在分析 {} ...\n", path.display());

    let tree = build_dir_tree(&path, 2)?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&tree)?);
        return Ok(());
    }

    print_tree(&tree, 0, threshold_bytes);

    let large_files = find_large_files(&tree, threshold_bytes);
    if !large_files.is_empty() {
        println!("\n大文件 (>= {}):\n", format_size(threshold_bytes, DECIMAL));
        for (path, size) in &large_files {
            println!("  {} — {}", path.display(), format_size(*size, DECIMAL));
        }
    }

    println!(
        "\n总计: {}",
        format_size(tree.size, DECIMAL),
    );

    Ok(())
}

fn build_dir_tree(path: &Path, max_depth: usize) -> Result<DirNode> {
    build_dir_tree_recursive(path, 0, max_depth)
}

fn build_dir_tree_recursive(path: &Path, depth: usize, max_depth: usize) -> Result<DirNode> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());

    let mut node = DirNode::new_dir(path.to_path_buf(), name);

    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => {
            log::debug!("无法读取目录 {:?}: {}", path, e);
            return Ok(node);
        }
    };

    let mut children: Vec<DirNode> = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let child_path = entry.path();
        let child_name = entry.file_name().to_string_lossy().to_string();

        if child_name.starts_with('.') {
            continue;
        }

        if meta.is_dir() {
            if depth < max_depth {
                let child = build_dir_tree_recursive(&child_path, depth + 1, max_depth)?;
                children.push(child);
            } else {
                let size = dir_size_fast(&child_path);
                let mut child = DirNode::new_dir(child_path, child_name);
                child.size = size;
                children.push(child);
            }
        } else if meta.is_file() {
            let size = meta.len();
            children.push(DirNode::new_file(child_path, child_name, size));
        }
    }

    children.sort_by_key(|c| std::cmp::Reverse(c.size));
    node.size = children.iter().map(|c| c.size).sum();
    node.children = children;

    Ok(node)
}

fn dir_size_fast(path: &Path) -> u64 {
    jwalk::WalkDir::new(path)
        .skip_hidden(false)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.path().symlink_metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

fn print_tree(node: &DirNode, indent: usize, threshold: u64) {
    let prefix = "  ".repeat(indent);
    let size_str = format_size(node.size, DECIMAL);

    if node.is_file {
        let marker = if node.size >= threshold { " !!!" } else { "" };
        println!("{}{} — {}{}", prefix, node.name, size_str, marker);
    } else {
        println!("{}{} — {}", prefix, node.name, size_str);
        let top_n = 20;
        for child in node.children.iter().take(top_n) {
            print_tree(child, indent + 1, threshold);
        }
        if node.children.len() > top_n {
            println!("{}  ... 和 {} 个其他项", prefix, node.children.len() - top_n);
        }
    }
}

fn find_large_files(node: &DirNode, threshold: u64) -> Vec<(PathBuf, u64)> {
    let mut result = Vec::new();
    find_large_files_recursive(node, threshold, &mut result);
    result.sort_by_key(|item| std::cmp::Reverse(item.1));
    result
}

fn find_large_files_recursive(node: &DirNode, threshold: u64, result: &mut Vec<(PathBuf, u64)>) {
    if node.is_file && node.size >= threshold {
        result.push((node.path.clone(), node.size));
    }
    for child in &node.children {
        find_large_files_recursive(child, threshold, result);
    }
}
