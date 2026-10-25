use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::Path;

#[cfg(unix)]
use diskclean_core::NodeKind;
use diskclean_core::{ScanControl, ScanError, ScanOptions, Tree, scan};
use tempfile::TempDir;

fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("create temp dir")
}

fn write_file(path: &Path, size: usize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directory");
    }
    let mut file = fs::File::create(path).expect("create file");
    file.write_all(&vec![b'x'; size]).expect("write file");
}

fn scan_tree(root: &Path, options: &ScanOptions) -> Tree {
    scan(root, options, &ScanControl::new()).expect("scan succeeds")
}

fn find_node<'a>(tree: &'a Tree, name: &str) -> Option<&'a diskclean_core::Node> {
    tree.nodes.iter().find(|node| node.name == OsStr::new(name))
}

#[test]
fn counts_files_directories_and_sizes() {
    let temp = temp_dir();
    let root = temp.path();
    write_file(&root.join("a.bin"), 100);
    write_file(&root.join("sub").join("b.bin"), 250);
    fs::create_dir(root.join("empty")).expect("create empty dir");

    let tree = scan_tree(root, &ScanOptions::default());

    assert_eq!(tree.stats.files, 2);
    assert_eq!(tree.stats.directories, 3);
    assert_eq!(tree.stats.logical_bytes, 350);
    assert!(tree.stats.allocated_bytes >= 350);

    let root_node = tree.node(tree.root());
    assert_eq!(root_node.file_count, 2);
    assert_eq!(root_node.dir_count, 3);
    assert_eq!(root_node.logical_size, 350);

    let top = tree.top_file_summaries();
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].logical_size, 250);
    assert_eq!(top[0].path, root.join("sub").join("b.bin"));
}

#[cfg(unix)]
#[test]
fn hardlinks_are_counted_once() {
    let temp = temp_dir();
    let root = temp.path();
    let original = root.join("original.bin");
    write_file(&original, 100);
    fs::hard_link(&original, root.join("link.bin")).expect("create hardlink");

    let tree = scan_tree(root, &ScanOptions::default());

    assert_eq!(tree.stats.files, 2);
    assert_eq!(tree.stats.hardlink_duplicates, 1);
    assert_eq!(tree.stats.logical_bytes, 100);
    assert_eq!(tree.node(tree.root()).logical_size, 100);
    assert_eq!(tree.node(tree.root()).file_count, 2);

    let summaries = tree.top_file_summaries();
    assert_eq!(summaries.len(), 2);
    assert_eq!(
        summaries
            .iter()
            .filter(|file| file.is_hardlink_duplicate)
            .count(),
        1
    );
    assert_eq!(
        summaries.iter().map(|file| file.logical_size).sum::<u64>(),
        200
    );
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed_or_counted_as_data() {
    use std::os::unix::fs::symlink;

    let temp = temp_dir();
    let root = temp.path();
    let target_dir = root.join("target-dir");
    write_file(&target_dir.join("data.bin"), 50);
    symlink(&target_dir, root.join("link-dir")).expect("create dir symlink");
    symlink(target_dir.join("data.bin"), root.join("link-file")).expect("create file symlink");

    let tree = scan_tree(root, &ScanOptions::default());

    assert_eq!(tree.stats.files, 1);
    assert_eq!(tree.stats.symlinks, 2);
    assert_eq!(tree.stats.logical_bytes, 50);
    assert_eq!(tree.stats.directories, 2);

    let link_file = find_node(&tree, "link-file").expect("find link-file");
    assert_eq!(link_file.kind, NodeKind::Symlink);
    assert_eq!(link_file.actual_logical_size, 0);
    assert_eq!(link_file.logical_size, 0);

    let link_dir = find_node(&tree, "link-dir").expect("find link-dir");
    assert_eq!(link_dir.kind, NodeKind::Symlink);
    assert_eq!(link_dir.dir_count, 0);
}

#[test]
fn excluded_directories_are_not_entered() {
    let temp = temp_dir();
    let root = temp.path();
    write_file(&root.join("keep").join("keep.bin"), 10);
    write_file(&root.join("skip").join("skip.bin"), 20);

    let options = ScanOptions {
        exclude: vec![root.join("skip")],
        ..ScanOptions::default()
    };
    let tree = scan_tree(root, &options);

    assert_eq!(tree.stats.files, 1);
    assert_eq!(tree.stats.logical_bytes, 10);
    assert!(tree.stats.skipped_excluded >= 1);
    assert!(tree.nodes.iter().all(|node| node.name != "skip"));
}

#[test]
fn max_depth_limits_directories_but_keeps_shallow_files() {
    let temp = temp_dir();
    let root = temp.path();
    write_file(&root.join("top.bin"), 10);
    write_file(&root.join("level1").join("one.bin"), 20);
    write_file(&root.join("level1").join("level2").join("two.bin"), 30);

    let options = ScanOptions {
        max_depth: Some(1),
        ..ScanOptions::default()
    };
    let tree = scan_tree(root, &options);

    assert_eq!(tree.stats.files, 2);
    assert_eq!(tree.stats.directories, 2);
    assert_eq!(tree.stats.logical_bytes, 30);
    assert_eq!(tree.stats.skipped_depth, 1);
    assert!(find_node(&tree, "two.bin").is_none());
}

#[test]
fn top_files_limit_is_respected() {
    let temp = temp_dir();
    let root = temp.path();
    write_file(&root.join("small.bin"), 10);
    write_file(&root.join("large.bin"), 100);
    write_file(&root.join("medium.bin"), 50);

    let options = ScanOptions {
        top_files: 1,
        ..ScanOptions::default()
    };
    let tree = scan_tree(root, &options);

    let top = tree.top_file_summaries();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].logical_size, 100);
    assert_eq!(top[0].path, root.join("large.bin"));
}

#[test]
fn hidden_entries_can_be_skipped() {
    let temp = temp_dir();
    let root = temp.path();
    write_file(&root.join(".hidden"), 10);
    write_file(&root.join("visible"), 20);

    let options = ScanOptions {
        skip_hidden: true,
        ..ScanOptions::default()
    };
    let tree = scan_tree(root, &options);

    assert_eq!(tree.stats.files, 1);
    assert_eq!(tree.stats.logical_bytes, 20);
    assert_eq!(tree.stats.skipped_excluded, 1);
    assert!(find_node(&tree, ".hidden").is_none());
}

#[test]
fn cancellation_before_scan_returns_cancelled() {
    let temp = temp_dir();
    write_file(&temp.path().join("file.bin"), 1);

    let control = ScanControl::new();
    control.cancel();
    let result = scan(temp.path(), &ScanOptions::default(), &control);

    assert!(matches!(result, Err(ScanError::Cancelled)));
}

#[test]
fn report_contains_tree_and_summaries() {
    let temp = temp_dir();
    let root = temp.path();
    write_file(&root.join("a.bin"), 10);

    let tree = scan_tree(root, &ScanOptions::default());
    let report = tree.report(true);
    let json = serde_json::to_value(&report).expect("serialize report");

    assert_eq!(json["version"], 1);
    assert_eq!(json["stats"]["files"], 1);
    assert!(json["tree"].is_object());
    assert_eq!(
        json["top_files"][0]["path"],
        root.join("a.bin").to_string_lossy().as_ref()
    );
}
