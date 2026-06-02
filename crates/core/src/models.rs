use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyLevel {
    Safe,
    Moderate,
    Risky,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeleteMode {
    Trash,
    Permanent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanItem {
    pub path: PathBuf,
    pub size: u64,
    pub safety: SafetyLevel,
    pub category: String,
    pub selected: bool,
}

impl ScanItem {
    pub fn new(path: PathBuf, size: u64, safety: SafetyLevel, category: String) -> Self {
        Self {
            path,
            size,
            safety,
            selected: safety == SafetyLevel::Safe,
            category,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryGroup {
    pub name: String,
    pub items: Vec<ScanItem>,
    pub total_size: u64,
    pub file_count: usize,
}

impl CategoryGroup {
    pub fn new(name: String, items: Vec<ScanItem>) -> Self {
        let total_size = items.iter().map(|i| i.size).sum();
        let file_count = items.len();
        Self {
            name,
            items,
            total_size,
            file_count,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanResult {
    pub categories: Vec<CategoryGroup>,
    pub total_size: u64,
    pub file_count: usize,
}

impl ScanResult {
    pub fn from_categories(categories: Vec<CategoryGroup>) -> Self {
        let total_size = categories.iter().map(|c| c.total_size).sum();
        let file_count = categories.iter().map(|c| c.file_count).sum();
        Self {
            categories,
            total_size,
            file_count,
        }
    }

    pub fn selected_items(&self) -> Vec<&ScanItem> {
        self.categories
            .iter()
            .flat_map(|c| c.items.iter())
            .filter(|i| i.selected)
            .collect()
    }

    pub fn selected_size(&self) -> u64 {
        self.selected_items().iter().map(|i| i.size).sum()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanedItem {
    pub path: PathBuf,
    pub size: u64,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanReport {
    pub cleaned: Vec<CleanedItem>,
    pub total_freed: u64,
    pub success_count: usize,
    pub failure_count: usize,
}

impl CleanReport {
    pub fn add(&mut self, item: CleanedItem) {
        if item.success {
            self.total_freed += item.size;
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }
        self.cleaned.push(item);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub bundle_id: Option<String>,
    pub path: PathBuf,
    pub size: u64,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirNode {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub children: Vec<DirNode>,
    pub is_file: bool,
}

impl DirNode {
    pub fn new_dir(path: PathBuf, name: String) -> Self {
        Self {
            path,
            name,
            size: 0,
            children: Vec::new(),
            is_file: false,
        }
    }

    pub fn new_file(path: PathBuf, name: String, size: u64) -> Self {
        Self {
            path,
            name,
            size,
            children: Vec::new(),
            is_file: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Clean,
    Uninstall,
    Analyze,
    Purge,
}
