use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::cli_app::CliApp;
use crate::package_owner::PackageOwner;

/// CLI binaries with a shared package uninstall command.
pub(crate) struct CliGroup {
    pub(crate) name: String,
    pub(crate) binaries: Vec<CliApp>,
    pub(crate) uninstall_command: Option<String>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum GroupKey {
    Package(String),
    Unowned(PathBuf),
}

impl CliGroup {
    /// Discovers CLI groups, excluding executables already shown in Desktop apps.
    pub(crate) fn installed(desktop_executables: &[PathBuf]) -> Vec<Self> {
        let apps = CliApp::installed();
        let paths = apps
            .iter()
            .map(|app| &app.executable)
            .chain(desktop_executables)
            .flat_map(|path| std::iter::once(path.clone()).chain(path.canonicalize().ok()))
            .collect();
        let owners = PackageOwner::lookup(&paths);
        Self::group(
            Self::without_desktop(apps, desktop_executables, &owners),
            &owners,
        )
    }

    fn without_desktop(
        apps: Vec<CliApp>,
        desktop_executables: &[PathBuf],
        owners: &BTreeMap<PathBuf, PackageOwner>,
    ) -> Vec<CliApp> {
        let desktop_paths: BTreeSet<_> = desktop_executables
            .iter()
            .flat_map(|path| std::iter::once(path.clone()).chain(path.canonicalize().ok()))
            .collect();
        // A wrapper script can have a different path from its desktop executable.
        // Require both the same basename and owner so unrelated CLI tools survive.
        let desktop_commands: BTreeSet<_> = desktop_paths
            .iter()
            .filter_map(|path| {
                Some((
                    path.file_name()?,
                    Self::owner(path, owners)?.command.as_str(),
                ))
            })
            .collect();
        apps.into_iter()
            .filter(|app| {
                !desktop_paths.contains(&app.executable)
                    && !app
                        .executable
                        .canonicalize()
                        .ok()
                        .is_some_and(|path| desktop_paths.contains(&path))
                    && !Self::owner(&app.executable, owners).is_some_and(|owner| {
                        app.executable.file_name().is_some_and(|name| {
                            desktop_commands.contains(&(name, owner.command.as_str()))
                        })
                    })
            })
            .collect()
    }

    fn owner<'a>(
        path: &Path,
        owners: &'a BTreeMap<PathBuf, PackageOwner>,
    ) -> Option<&'a PackageOwner> {
        owners
            .get(path)
            .or_else(|| owners.get(&path.canonicalize().ok()?))
    }

    fn group(apps: Vec<CliApp>, owners: &BTreeMap<PathBuf, PackageOwner>) -> Vec<Self> {
        let mut groups = BTreeMap::new();
        for app in apps {
            let owner = Self::owner(&app.executable, owners);
            let key = match owner {
                Some(owner) => GroupKey::Package(owner.command.clone()),
                None => GroupKey::Unowned(app.executable.clone()),
            };
            let group = groups.entry(key).or_insert_with(|| Self {
                name: owner.map_or_else(|| app.name.clone(), |owner| owner.name.clone()),
                binaries: Vec::new(),
                uninstall_command: owner.map(|owner| owner.command.clone()),
            });
            group.binaries.push(app);
        }
        let mut groups: Vec<_> = groups.into_values().collect();
        for group in &mut groups {
            group
                .binaries
                .sort_by_cached_key(|app| app.name.to_lowercase());
        }
        groups.sort_by_cached_key(|group| group.name.to_lowercase());
        groups
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    use super::CliGroup;
    use crate::cli_app::CliApp;
    use crate::package_owner::PackageOwner;

    fn app(name: &str) -> CliApp {
        CliApp {
            name: name.into(),
            executable: PathBuf::from("/example/bin").join(name),
        }
    }

    #[test]
    fn groups_owned_binaries_and_keeps_unknown_installations_separate() {
        let apps = vec![
            app("yuvdeinterlace"),
            app("custom"),
            app("yuvcorrect_tune"),
            app("another"),
        ];
        let owners = ["yuvdeinterlace", "yuvcorrect_tune"]
            .into_iter()
            .map(|name| {
                (
                    app(name).executable,
                    PackageOwner {
                        name: "mjpegtools".into(),
                        command: "sudo pacman -R -- mjpegtools".into(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let groups = CliGroup::group(apps, &owners);
        assert_eq!(groups.len(), 3);
        let group = groups
            .iter()
            .find(|group| group.name == "mjpegtools")
            .expect("package group");
        assert_eq!(
            group
                .binaries
                .iter()
                .map(|app| app.name.as_str())
                .collect::<Vec<_>>(),
            ["yuvcorrect_tune", "yuvdeinterlace"]
        );
        assert_eq!(
            group.uninstall_command.as_deref(),
            Some("sudo pacman -R -- mjpegtools")
        );
        assert!(
            groups
                .iter()
                .filter(|group| group.name != "mjpegtools")
                .all(|group| group.uninstall_command.is_none()),
            "unowned commands must not share a guessed uninstall command"
        );
    }

    #[test]
    fn hides_desktop_executables_but_retains_other_binaries() {
        let apps = CliGroup::without_desktop(
            vec![app("firefox"), app("tool")],
            &[app("firefox").executable],
            &BTreeMap::new(),
        );
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "tool");
    }

    #[test]
    fn desktop_wrappers_with_the_same_name_and_owner_are_hidden() {
        let desktop = PathBuf::from("/example/lib/firefox/firefox");
        let owners = [
            app("firefox").executable,
            app("helper").executable,
            desktop.clone(),
        ]
        .into_iter()
        .map(|path| {
            (
                path,
                PackageOwner {
                    name: "firefox".into(),
                    command: "sudo pacman -R -- firefox".into(),
                },
            )
        })
        .collect();
        // Unlike a symlink, a wrapper script has a different resolved path.
        let visible =
            CliGroup::without_desktop(vec![app("firefox"), app("helper")], &[desktop], &owners);
        let groups = CliGroup::group(visible, &owners);
        assert_eq!(
            groups[0]
                .binaries
                .iter()
                .map(|app| app.name.as_str())
                .collect::<Vec<_>>(),
            ["helper"]
        );
    }

    #[test]
    fn matching_names_from_different_packages_remain_visible() {
        let desktop = PathBuf::from("/example/lib/firefox/firefox");
        let owners = BTreeMap::from([
            (
                desktop.clone(),
                PackageOwner {
                    name: "desktop-package".into(),
                    command: "sudo pacman -R -- desktop-package".into(),
                },
            ),
            (
                app("firefox").executable,
                PackageOwner {
                    name: "cli-package".into(),
                    command: "sudo pacman -R -- cli-package".into(),
                },
            ),
        ]);
        let visible = CliGroup::without_desktop(vec![app("firefox")], &[desktop], &owners);
        assert_eq!(
            visible.len(),
            1,
            "matching command names alone must not hide unrelated executables"
        );
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn aliases_use_the_target_owner_and_do_not_duplicate_desktop_apps() {
        let fixture = Fixture {
            root: std::env::temp_dir().join(format!("pacat-group-test-{}", std::process::id())),
        };
        fs::create_dir_all(&fixture.root).expect("create fixture directory");
        let target = fixture.root.join("executable");
        let link = fixture.root.join("alias");
        fs::write(&target, "fixture").expect("create target");
        symlink(&target, &link).expect("create executable alias");
        let apps = || {
            vec![
                CliApp {
                    name: "executable".into(),
                    executable: target.clone(),
                },
                CliApp {
                    name: "alias".into(),
                    executable: link.clone(),
                },
            ]
        };
        let owners = BTreeMap::from([(
            target.canonicalize().expect("resolve target"),
            PackageOwner {
                name: "example".into(),
                command: "sudo pacman -R -- example".into(),
            },
        )]);
        let groups = CliGroup::group(apps(), &owners);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].binaries.len(), 2);
        assert!(
            CliGroup::without_desktop(apps(), &[link], &owners).is_empty(),
            "desktop aliases must hide both the alias and resolved executable"
        );
    }
}
