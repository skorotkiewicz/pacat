use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// An executable available through the application's PATH.
pub(crate) struct CliApp {
    pub(crate) name: String,
    pub(crate) executable: PathBuf,
}

impl CliApp {
    /// Finds commands without running them. The first executable with each name wins.
    pub(crate) fn installed() -> Vec<Self> {
        let Some(path) = env::var_os("PATH") else {
            return Vec::new();
        };
        Self::from_directories(env::split_paths(&path))
    }

    fn from_directories(directories: impl IntoIterator<Item = PathBuf>) -> Vec<Self> {
        let mut commands = BTreeMap::new();
        for directory in directories {
            // Empty and relative PATH entries are relative to the working directory.
            let directory = if directory.is_absolute() {
                directory
            } else if let Ok(current) = env::current_dir() {
                current.join(directory)
            } else {
                continue;
            };
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.filter_map(Result::ok) {
                let executable = entry.path();
                let Ok(metadata) = fs::metadata(&executable) else {
                    continue;
                };
                if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                    commands.entry(entry.file_name()).or_insert_with(|| Self {
                        name: entry.file_name().to_string_lossy().into_owned(),
                        executable,
                    });
                }
            }
        }
        let mut apps: Vec<_> = commands.into_values().collect();
        apps.sort_by_cached_key(|app| app.name.to_lowercase());
        apps
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::PathBuf;

    use super::CliApp;

    struct Fixture(PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn discovers_executables_and_symlinks_in_path_order_without_running_them() {
        let fixture =
            Fixture(std::env::temp_dir().join(format!("pacat-cli-test-{}", std::process::id())));
        let first = fixture.0.join("first");
        let second = fixture.0.join("second");
        fs::create_dir_all(&first).expect("create first PATH directory");
        fs::create_dir_all(&second).expect("create second PATH directory");
        for path in [first.join("opencode"), second.join("opencode")] {
            fs::write(&path, "not a runnable program").expect("create executable fixture");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .expect("mark fixture executable");
        }
        fs::write(first.join("notes"), "plain text").expect("create non-executable");
        fs::create_dir(first.join("directory")).expect("create directory");
        symlink(first.join("opencode"), first.join("alias")).expect("create executable symlink");
        symlink(first.join("missing"), first.join("broken")).expect("create broken symlink");

        let apps = CliApp::from_directories([
            first.clone(),
            second,
            first.clone(),
            fixture.0.join("unreadable-or-missing"),
        ]);
        assert_eq!(
            apps.iter().map(|app| app.name.as_str()).collect::<Vec<_>>(),
            ["alias", "opencode"]
        );
        assert_eq!(apps[1].executable, first.join("opencode"));
    }
}
