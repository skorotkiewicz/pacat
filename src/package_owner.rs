use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The package names and shared uninstall command for an executable.
pub(crate) struct PackageOwner {
    pub(crate) name: String,
    pub(crate) command: String,
}

impl PackageOwner {
    /// Queries ownership in batches, retaining matches even when other files are unowned.
    pub(crate) fn lookup(paths: &BTreeSet<PathBuf>) -> BTreeMap<PathBuf, Self> {
        let mut owners = BTreeMap::new();
        for (program, args, format) in [
            ("pacman", &["-Qo", "--"][..], Format::Pacman),
            ("dpkg-query", &["-S", "--"][..], Format::Debian),
        ] {
            let pending: Vec<_> = paths
                .iter()
                .filter(|path| !owners.contains_key(*path))
                .collect();
            for batch in pending.chunks(128) {
                let Ok(output) = Command::new(program)
                    .args(args)
                    .args(batch)
                    .env("LC_ALL", "C")
                    .stdin(Stdio::null())
                    .stderr(Stdio::null())
                    .output()
                else {
                    break;
                };
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    if let Some((path, owner)) = format.parse(line)
                        && paths.contains(&path)
                    {
                        owners.entry(path).or_insert(owner);
                    }
                }
            }
        }
        // RPM ownership output does not associate paths with package names, so
        // query its remaining files individually, only when RPM is available.
        if gtk4::glib::find_program_in_path("rpm").is_some() {
            for path in paths
                .iter()
                .filter(|path| !owners.contains_key(*path))
                .collect::<Vec<_>>()
            {
                if let Some(owner) = Self::rpm(path) {
                    owners.insert(path.clone(), owner);
                }
            }
        }
        owners
    }

    fn rpm(path: &Path) -> Option<Self> {
        let output = Command::new("rpm")
            .args(["-qf", "--queryformat", "%{NAME}\n", "--"])
            .arg(path)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Self::new(
            String::from_utf8_lossy(&output.stdout).lines(),
            "sudo rpm -e --",
        )
    }

    fn new<'a>(packages: impl Iterator<Item = &'a str>, prefix: &str) -> Option<Self> {
        let packages: BTreeSet<_> = packages.map(str::trim).collect();
        if packages.is_empty()
            || packages.iter().any(|package| {
                package.is_empty()
                    || package.starts_with('-')
                    || !package
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_+-.:@".contains(&byte))
            })
        {
            return None;
        }
        Some(Self {
            name: packages.iter().copied().collect::<Vec<_>>().join(", "),
            command: format!(
                "{prefix} {}",
                packages.into_iter().collect::<Vec<_>>().join(" ")
            ),
        })
    }
}

#[derive(Clone, Copy)]
enum Format {
    Pacman,
    Debian,
}

impl Format {
    fn parse(self, line: &str) -> Option<(PathBuf, PackageOwner)> {
        let (path, owner) = match self {
            Self::Pacman => {
                let (path, package) = line.rsplit_once(" is owned by ")?;
                let (package, _) = package.split_once(' ')?;
                (
                    path,
                    PackageOwner::new(std::iter::once(package), "sudo pacman -R --")?,
                )
            }
            Self::Debian => {
                let (packages, path) = line.split_once(": ")?;
                (
                    path,
                    PackageOwner::new(packages.split(", "), "sudo apt remove --")?,
                )
            }
        };
        Some((PathBuf::from(path), owner))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Format;

    #[test]
    fn pacman_groups_different_binaries_by_package_instead_of_filename() {
        let (first_path, first) = Format::Pacman
            .parse("/usr/bin/yuvcorrect_tune is owned by mjpegtools 2.2.1-1")
            .expect("ownership line");
        let (_, second) = Format::Pacman
            .parse("/usr/bin/yuvdeinterlace is owned by mjpegtools 2.2.1-1")
            .expect("ownership line");
        assert_eq!(first_path, Path::new("/usr/bin/yuvcorrect_tune"));
        assert_eq!(first.name, "mjpegtools");
        assert_eq!(first.command, "sudo pacman -R -- mjpegtools");
        assert_eq!(first.command, second.command);
    }

    #[test]
    fn debian_preserves_architectures_and_normalizes_multiple_owners() {
        let (path, owner) = Format::Debian
            .parse("zlib1g:amd64, example, example: /usr/bin/tool with spaces")
            .expect("ownership line");
        assert_eq!(path, Path::new("/usr/bin/tool with spaces"));
        assert_eq!(owner.command, "sudo apt remove -- example zlib1g:amd64");
    }

    #[test]
    fn errors_and_unsafe_package_names_are_not_commands() {
        for line in [
            "error: No package owns /usr/bin/custom",
            "/usr/bin/tool is owned by -bad 1.0",
            "/usr/bin/tool is owned by $(bad) 1.0",
        ] {
            assert!(
                Format::Pacman.parse(line).is_none(),
                "invalid ownership accepted: {line}"
            );
        }
        assert!(
            Format::Debian
                .parse("diversion by example from: /usr/bin/tool")
                .is_none(),
            "diversion metadata must not become a package name"
        );
    }
}
