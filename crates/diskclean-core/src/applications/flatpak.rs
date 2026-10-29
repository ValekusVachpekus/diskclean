//! Active Flatpak deployments, with runtimes retained as separate records.
use super::*;
use diskclean_platform::flatpak;

const MAX_ENTRIES: usize = 20_000;
const MAX_BYTES: u64 = 64 * 1024 * 1024;

struct Inventory<'a> {
    options: &'a ScanOptions,
    control: &'a ScanControl,
    mounts: MountPoints,
    report: ApplicationsReport,
    entries: usize,
    bytes: u64,
    sizes: SizeCache,
}
impl Inventory<'_> {
    fn issue(&mut self, path: PathBuf, error: std::io::Error) {
        self.report
            .issues
            .push(ScanIssue::from_io(path, "read Flatpak deployment", &error));
    }
    /// Only three directory levels: ID / architecture / branch. Never recurse
    /// into repo, exports, .removed, current aliases or arbitrary symlink targets.
    fn children(
        &mut self,
        root: &Path,
        path: &Path,
        optional: bool,
    ) -> Result<Vec<PathBuf>, ScanError> {
        let result =
            permitted(root, path, self.options, &self.mounts).and_then(|()| fs::read_dir(path));
        let entries = match result {
            Ok(entries) => entries,
            Err(error) if optional && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => {
                self.issue(path.into(), error);
                return Ok(Vec::new());
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            if self.control.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            if self.entries >= MAX_ENTRIES {
                self.issue(
                    path.into(),
                    std::io::Error::other("Flatpak directory entry limit reached"),
                );
                break;
            }
            self.entries += 1;
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(error) => {
                    self.issue(path.into(), error);
                    continue;
                }
            };
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if name == "current" {
                continue;
            } // Well-known app branch alias.
            if !flatpak::valid_component(name) || name.starts_with('.') {
                continue;
            }
            match permitted(root, &path, self.options, &self.mounts).and_then(|()| metadata(&path))
            {
                Ok(meta) if meta.kind == FileKind::Directory => paths.push(path),
                Ok(_) => self.issue(path, std::io::ErrorKind::NotADirectory.into()),
                Err(error) => self.issue(path, error),
            }
        }
        paths.sort();
        Ok(paths)
    }
    fn deployment(&mut self, root: &Path, branch: &Path, runtime: bool) -> Result<(), ScanError> {
        let evidence = branch.join("active");
        let result = (|| -> std::io::Result<_> {
            let active = flatpak::read_active(branch)?;
            let deploy = branch.join(&active);
            let manifest = deploy.join("metadata");
            permitted(root, &manifest, self.options, &self.mounts)?;
            let meta = metadata(&manifest)?;
            if meta.logical_size > MAX_BYTES - self.bytes {
                return Err(std::io::Error::other("Flatpak metadata byte limit reached"));
            }
            self.bytes += meta.logical_size;
            let name = flatpak::read_name(&manifest, runtime, || self.control.is_cancelled())?;
            let id_dir = branch
                .parent()
                .and_then(Path::parent)
                .and_then(Path::file_name);
            if id_dir != Some(std::ffi::OsStr::new(&name)) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Flatpak identity does not match deployment directory",
                ));
            }
            let relative = branch.strip_prefix(root).map_err(std::io::Error::other)?;
            Ok((
                active,
                deploy,
                manifest,
                name,
                relative.to_string_lossy().replace('\\', "/"),
            ))
        })();
        if self.control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let (active, deploy, manifest, name, id) = match result {
            Ok(result) => result,
            Err(error) => {
                self.issue(evidence, error);
                return Ok(());
            }
        };
        let mut app = Application {
            id,
            name,
            launcher: if runtime {
                "Flatpak runtime"
            } else {
                "Flatpak"
            }
            .into(),
            status: InstallationStatus::Unknown,
            install_path: deploy,
            manifest_path: manifest,
            registry_key: None,
            reported_size_bytes: None,
            archive_path: None,
            snap_state: None,
            allocated_bytes: None,
            logical_bytes: None,
            size_complete: false,
            build_id: Some(active.clone()),
            last_played_unix: None,
        };
        measure_installation(
            &mut app,
            true,
            root,
            self.options,
            self.control,
            &self.mounts,
            &mut self.sizes,
            &mut self.report.issues,
        )?;
        // Updating Flatpak while measuring must not preserve a stale active claim.
        match flatpak::read_active(branch) {
            Ok(current) if current == active => {}
            _ => {
                app.status = InstallationStatus::Unknown;
                app.size_complete = false;
                self.issue(
                    evidence,
                    std::io::Error::other("Flatpak active deployment changed during inventory"),
                );
            }
        }
        self.report.applications.push(app);
        Ok(())
    }
}

pub fn discover_flatpak(
    roots: &[PathBuf],
    options: &ScanOptions,
    control: &ScanControl,
) -> Result<ApplicationsReport, ScanError> {
    if control.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    if options.follow_symlinks {
        return Err(ScanError::FollowSymlinksUnsupported);
    }
    let mounts = MountPoints::read().map_err(|source| ScanError::Root {
        path: "/".into(),
        source,
    })?;
    let mut inventory = Inventory {
        options,
        control,
        mounts,
        report: Default::default(),
        entries: 0,
        bytes: 0,
        sizes: Default::default(),
    };
    let mut visited = HashSet::new();
    for root in roots {
        if control.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let root = std::path::absolute(root).map_err(|source| ScanError::Root {
            path: root.clone(),
            source,
        })?;
        let meta = match permitted(&root, &root, options, &inventory.mounts)
            .and_then(|()| metadata(&root))
        {
            Ok(meta) if meta.kind == FileKind::Directory => meta,
            Ok(_) => {
                inventory.issue(root, std::io::ErrorKind::NotADirectory.into());
                continue;
            }
            Err(error) => {
                inventory.issue(root, error);
                continue;
            }
        };
        if meta.ino != 0 && !visited.insert((meta.dev, meta.ino)) {
            continue;
        }
        for (kind, runtime) in [("app", false), ("runtime", true)] {
            for id in inventory.children(&root, &root.join(kind), true)? {
                for arch in inventory.children(&root, &id, false)? {
                    for branch in inventory.children(&root, &arch, false)? {
                        inventory.deployment(&root, &branch, runtime)?;
                    }
                }
            }
        }
    }
    if control.is_cancelled() {
        return Err(ScanError::Cancelled);
    }
    inventory.report.applications.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.id.cmp(&b.id))
            .then(a.install_path.cmp(&b.install_path))
    });
    Ok(inventory.report)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    fn fixture(root: &Path, kind: &str, name: &str, branch_name: &str, suffix: &str) -> PathBuf {
        let branch = root.join(kind).join(name).join("x86_64").join(branch_name);
        let hash = format!("{}{suffix}", "a".repeat(64));
        let deploy = branch.join(&hash);
        fs::create_dir_all(deploy.join("files")).expect("dir");
        fs::write(
            deploy.join("metadata"),
            format!(
                "[{}]\nname={name}\n",
                if kind == "app" {
                    "Application"
                } else {
                    "Runtime"
                }
            ),
        )
        .expect("metadata");
        fs::write(deploy.join("files/content"), b"actual data").expect("content");
        symlink(hash, branch.join("active")).expect("active");
        deploy
    }
    #[test]
    fn inventory_has_active_apps_separate_shared_runtimes_and_actual_sizes() {
        let temp = tempfile::tempdir().expect("temp");
        let app = fixture(temp.path(), "app", "org.example.App", "stable", "");
        let runtime = fixture(
            temp.path(),
            "runtime",
            "org.example.Platform.Locale",
            "1",
            "-ru",
        );
        fixture(temp.path(), "app", "org.example.App", "beta", "");
        symlink(
            "x86_64/stable",
            temp.path().join("app/org.example.App/current"),
        )
        .expect("alias");
        let old = app.parent().expect("parent").join("b".repeat(64));
        fs::create_dir(&old).expect("old"); // Not active, not counted.
        fs::hard_link(app.join("files/content"), app.join("files/alias")).expect("hardlink");
        let report = discover_flatpak(
            &[temp.path().into(), temp.path().into()],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(report.applications.len(), 3);
        assert!(
            report
                .applications
                .iter()
                .all(|a| a.status == InstallationStatus::Installed
                    && a.allocated_bytes.is_some()
                    && a.reported_size_bytes.is_none())
        );
        let record = report
            .applications
            .iter()
            .find(|a| a.install_path == app)
            .expect("app");
        let tree = scan(&app, &ScanOptions::default(), &ScanControl::new()).expect("scan");
        assert_eq!(record.logical_bytes, Some(tree.nodes[0].logical_size));
        assert!(record.size_complete);
        assert_eq!(
            report
                .applications
                .iter()
                .find(|a| a.install_path == runtime)
                .expect("runtime")
                .launcher,
            "Flatpak runtime"
        );
        assert_eq!(
            fs::read_link(app.parent().expect("parent").join("active")).expect("unchanged"),
            PathBuf::from("a".repeat(64))
        );
    }
    #[test]
    fn invalid_active_and_linked_sources_preserve_valid_neighbours() {
        let temp = tempfile::tempdir().expect("temp");
        fixture(temp.path(), "app", "org.example.Valid", "stable", "");
        let bad = fixture(temp.path(), "app", "org.example.Bad", "stable", "");
        fs::remove_file(bad.join("metadata")).expect("remove fixture");
        symlink("/etc/passwd", bad.join("metadata")).expect("metadata link");
        let escape = temp.path().join("app/org.example.Escape/x86_64/stable");
        fs::create_dir_all(&escape).expect("dir");
        symlink("../../../../../outside", escape.join("active")).expect("escape");
        symlink(
            temp.path().join("app/org.example.Valid"),
            temp.path().join("app/org.example.Linked"),
        )
        .expect("dir link");
        let report = discover_flatpak(
            &[temp.path().into()],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert_eq!(report.applications.len(), 1);
        assert_eq!(report.applications[0].name, "org.example.Valid");
        assert_eq!(report.issues.len(), 3);
        let control = ScanControl::new();
        control.cancel();
        assert!(matches!(
            discover_flatpak(&[temp.path().into()], &ScanOptions::default(), &control),
            Err(ScanError::Cancelled)
        ));
    }
    #[test]
    fn identity_mismatch_and_dangling_active_are_not_installed_evidence() {
        let temp = tempfile::tempdir().expect("temp");
        let app = fixture(temp.path(), "app", "org.example.App", "stable", "");
        fs::write(
            app.join("metadata"),
            "[Application]\nname=org.example.Other\n",
        )
        .expect("identity");
        let runtime = fixture(temp.path(), "runtime", "org.example.Runtime", "1", "");
        fs::remove_file(runtime.parent().expect("parent").join("active")).expect("fixture");
        symlink(
            "b".repeat(64),
            runtime.parent().expect("parent").join("active"),
        )
        .expect("dangling");
        let report = discover_flatpak(
            &[temp.path().into()],
            &ScanOptions::default(),
            &ScanControl::new(),
        )
        .expect("report");
        assert!(report.applications.is_empty());
        assert_eq!(report.issues.len(), 2);
    }

    #[test]
    fn exhausted_metadata_and_directory_budgets_stop_inventory_without_scanning() {
        let temp = tempfile::tempdir().expect("temp");
        let deploy = fixture(temp.path(), "app", "org.example.App", "stable", "");
        let options = ScanOptions::default();
        let control = ScanControl::new();
        let mut inventory = Inventory {
            options: &options,
            control: &control,
            mounts: MountPoints::read().expect("mounts"),
            report: Default::default(),
            entries: MAX_ENTRIES,
            bytes: MAX_BYTES,
            sizes: Default::default(),
        };
        assert!(
            inventory
                .children(temp.path(), &temp.path().join("app"), false)
                .expect("bounded")
                .is_empty()
        );
        inventory
            .deployment(temp.path(), deploy.parent().expect("branch"), false)
            .expect("metadata bounded");
        assert_eq!(inventory.report.issues.len(), 2);
        assert!(inventory.report.applications.is_empty());
        assert_eq!(control.snapshot().files, 0);
    }
}
