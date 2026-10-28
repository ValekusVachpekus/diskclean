//! Immutable full-tree index and cached view queries.
use crate::app::{Filters, SortKey, SortState};
use diskclean_core::{ExtensionSummary, IssueKind, NodeId, NodeKind, Tree, parse_size};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
};

#[derive(Debug, Default)]
pub(crate) struct AnalysisIndex {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) names: Vec<String>,
    searchable: Vec<String>,
    extensions: Vec<String>,
    children: Vec<Vec<NodeId>>,
    issue_paths: Vec<String>,
}

impl AnalysisIndex {
    pub(crate) fn new(tree: &Tree) -> Self {
        let paths: Vec<_> = (0..tree.nodes.len())
            .map(|id| tree.path(id as NodeId))
            .collect();
        let mut index = Self {
            names: tree.nodes.iter().map(|node| node.display_name()).collect(),
            searchable: paths
                .iter()
                .map(|path| path.to_string_lossy().to_lowercase())
                .collect(),
            extensions: paths
                .iter()
                .map(|path| {
                    path.extension()
                        .map(|ext| ext.to_string_lossy().to_lowercase())
                        .unwrap_or_default()
                })
                .collect(),
            children: vec![Vec::new(); tree.nodes.len()],
            issue_paths: tree
                .issues
                .iter()
                .map(|issue| issue.path.to_string_lossy().to_lowercase())
                .collect(),
            paths,
        };
        for (id, node) in tree.nodes.iter().enumerate() {
            if let Some(parent) = node.parent {
                index.children[parent as usize].push(id as NodeId);
            }
        }
        for children in &mut index.children {
            children.sort_unstable_by(|a, b| {
                tree.node(*b)
                    .allocated_size
                    .cmp(&tree.node(*a).allocated_size)
                    .then_with(|| index.names[*a as usize].cmp(&index.names[*b as usize]))
            });
        }
        index
    }

    fn sort(&self, tree: &Tree, ids: &mut [NodeId], sort: SortState) {
        ids.sort_unstable_by(|a, b| {
            let left = tree.node(*a);
            let right = tree.node(*b);
            let order = match sort.key {
                SortKey::Name => self.names[*a as usize].cmp(&self.names[*b as usize]),
                SortKey::Size => left.allocated_size.cmp(&right.allocated_size),
                SortKey::Modified => left.modified_unix.cmp(&right.modified_unix),
                SortKey::Count => left.file_count.cmp(&right.file_count),
            };
            (if sort.descending {
                order.reverse()
            } else {
                order
            })
            .then_with(|| self.paths[*a as usize].cmp(&self.paths[*b as usize]))
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnalysisQuery {
    pub(crate) filters: Filters,
    pub(crate) files: SortState,
    pub(crate) folders: SortState,
    pub(crate) types: SortState,
    pub(crate) issue_kind: Option<IssueKind>,
}

#[derive(Default)]
pub(crate) struct AnalysisView {
    key: Option<AnalysisQuery>,
    pub(crate) files: Arc<Vec<NodeId>>,
    pub(crate) folders: Arc<Vec<NodeId>>,
    pub(crate) extensions: Arc<Vec<ExtensionSummary>>,
    pub(crate) issues: Arc<Vec<usize>>,
    matches: Vec<bool>,
    children: HashMap<NodeId, Arc<Vec<NodeId>>>,
}

impl AnalysisView {
    pub(crate) fn update(&mut self, tree: &Tree, index: &AnalysisIndex, query: AnalysisQuery) {
        if self.key.as_ref() == Some(&query) {
            return;
        }
        let needle = query.filters.search.trim().to_lowercase();
        let minimum = parse_size(query.filters.min_size.trim()).unwrap_or(0);
        let mut files = Vec::new();
        let mut folders = Vec::new();
        let mut extensions: BTreeMap<String, ExtensionSummary> = BTreeMap::new();
        self.matches = vec![false; tree.nodes.len()];
        for (id, node) in tree.nodes.iter().enumerate().skip(1) {
            if node.allocated_size < minimum || !index.searchable[id].contains(&needle) {
                continue;
            }
            let visible = if node.kind.is_dir() {
                query.filters.show_dirs
            } else {
                query.filters.show_files
            };
            self.matches[id] = visible;
            if !visible {
                continue;
            }
            if node.kind.is_dir() {
                folders.push(id as NodeId);
            }
            if node.kind == NodeKind::File {
                files.push(id as NodeId);
                let extension = index.extensions[id].clone();
                let entry = extensions
                    .entry(extension.clone())
                    .or_insert(ExtensionSummary {
                        extension,
                        files: 0,
                        logical_bytes: 0,
                        allocated_bytes: 0,
                    });
                entry.files += 1;
                entry.logical_bytes = entry.logical_bytes.saturating_add(node.logical_size);
                entry.allocated_bytes = entry.allocated_bytes.saturating_add(node.allocated_size);
            }
        }
        index.sort(tree, &mut files, query.files);
        index.sort(tree, &mut folders, query.folders);
        let mut extensions: Vec<_> = extensions.into_values().collect();
        extensions.sort_unstable_by(|a, b| {
            let order = match query.types.key {
                SortKey::Name => a.extension.cmp(&b.extension),
                SortKey::Count => a.files.cmp(&b.files),
                _ => a.allocated_bytes.cmp(&b.allocated_bytes),
            };
            (if query.types.descending {
                order.reverse()
            } else {
                order
            })
            .then_with(|| a.extension.cmp(&b.extension))
        });
        self.issues = Arc::new(
            tree.issues
                .iter()
                .enumerate()
                .filter(|(id, issue)| {
                    index.issue_paths[*id].contains(&needle)
                        && query.issue_kind.is_none_or(|kind| kind == issue.kind)
                })
                .map(|(id, _)| id)
                .collect(),
        );
        self.files = Arc::new(files);
        self.folders = Arc::new(folders);
        self.extensions = Arc::new(extensions);
        self.children.clear();
        self.key = Some(query);
    }

    pub(crate) fn children(&mut self, index: &AnalysisIndex, parent: NodeId) -> Arc<Vec<NodeId>> {
        Arc::clone(self.children.entry(parent).or_insert_with(|| {
            Arc::new(
                index.children[parent as usize]
                    .iter()
                    .copied()
                    .filter(|id| self.matches[*id as usize])
                    .collect(),
            )
        }))
    }
}
