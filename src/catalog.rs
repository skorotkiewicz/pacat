use std::io;
use std::process::{Command, Stdio};

use crate::package::{Package, Source};

pub(crate) struct Catalog {
    pub(crate) packages: Vec<Package>,
    pub(crate) warnings: Vec<String>,
}

impl Catalog {
    pub(crate) fn scan() -> io::Result<Self> {
        if !cfg!(target_os = "linux") {
            return Err(io::Error::other("pacat currently supports Linux"));
        }
        let mut catalog = Self {
            packages: Vec::new(),
            warnings: Vec::new(),
        };
        let mut available = 0;
        // Fixed, read-only queries. No shell, sudo, or uninstall command is run.
        let queries: &[(&str, &[&str])] = &[
            ("pacman", &["-Q"]),
            (
                "dpkg-query",
                &[
                    "-W",
                    "-f=${binary:Package}\t${Version}\t${db:Status-Status}\\n",
                ],
            ),
            (
                "rpm",
                &[
                    "-qa",
                    "--queryformat",
                    "%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\t%{NAME}.%{ARCH}\t%{VERSION}-%{RELEASE}\\n",
                ],
            ),
            (
                "flatpak",
                &[
                    "list",
                    "--app",
                    "--columns=ref:full,name:full,version:full,installation:full",
                ],
            ),
            ("snap", &["list"]),
        ];
        for (program, args) in queries {
            let output = Command::new(program)
                .args(*args)
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .output();
            match output {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    available += 1;
                    catalog.warnings.push(format!("{program}: {error}"));
                }
                Ok(output) => {
                    available += 1;
                    if output.status.success() {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        catalog.packages.extend(parse(program, &stdout));
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let message = stderr.lines().next().unwrap_or("query failed");
                        catalog.warnings.push(format!("{program}: {message}"));
                    }
                }
            }
        }
        if available == 0 {
            return Err(io::Error::other(
                "no supported package manager found; supported: pacman, dpkg-query, rpm, Flatpak, Snap",
            ));
        }
        if catalog.packages.is_empty() && !catalog.warnings.is_empty() {
            return Err(io::Error::other(catalog.warnings.join("; ")));
        }
        catalog.packages.sort_by_cached_key(|package| {
            (
                package.name.to_lowercase(),
                package.source.to_string(),
                package.id.clone(),
            )
        });
        Ok(catalog)
    }
}

fn parse(program: &str, output: &str) -> Vec<Package> {
    output
        .lines()
        .filter_map(|line| match program {
            "pacman" => {
                let mut fields = line.split_whitespace();
                let id = fields.next()?;
                Package::new(id, id, fields.next()?, Source::Pacman)
            }
            "dpkg-query" => {
                let fields: Vec<_> = line.split('\t').collect();
                if fields.len() != 3 || fields[2] != "installed" {
                    return None;
                }
                Package::new(fields[0], fields[0], fields[1], Source::Debian)
            }
            "rpm" => {
                let fields: Vec<_> = line.split('\t').collect();
                if fields.len() != 3 {
                    return None;
                }
                Package::new(fields[0], fields[1], fields[2], Source::Rpm)
            }
            "flatpak" => {
                let fields: Vec<_> = line.split('\t').collect();
                if fields.len() != 4
                    || fields[3].is_empty()
                    || fields[3].chars().any(char::is_control)
                {
                    return None;
                }
                let name = if fields[1].is_empty() {
                    fields[0]
                } else {
                    fields[1]
                };
                Package::new(
                    fields[0],
                    name,
                    fields[2],
                    Source::Flatpak {
                        installation: fields[3].into(),
                    },
                )
            }
            "snap" => {
                let mut fields = line.split_whitespace();
                let id = fields.next()?;
                if id == "Name" {
                    return None;
                }
                Package::new(id, id, fields.next()?, Source::Snap)
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn debian_excludes_removed_and_incomplete_packages() {
        let packages = parse(
            "dpkg-query",
            "firefox\t1\tinstalled\nold\t2\tconfig-files\nbroken\t3\tunpacked\nlibc6:amd64\t4\tinstalled\n",
        );
        assert_eq!(packages.len(), 2);
        assert_eq!(
            packages[1].uninstall_command(),
            "sudo apt remove -- libc6:amd64"
        );
    }

    #[test]
    fn flatpak_preserves_spaces_empty_versions_and_duplicate_installations() {
        let packages = parse(
            "flatpak",
            "app/org.example.App/x86_64/stable\tExample App\t\tuser\napp/org.example.App/x86_64/stable\tExample App\t1\tsystem\n",
        );
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "Example App");
        assert_eq!(packages[0].version, "");
        assert_ne!(
            packages[0].uninstall_command(),
            packages[1].uninstall_command()
        );
    }

    #[test]
    fn native_and_snap_queries_parse_expected_records() {
        assert_eq!(
            parse("pacman", "firefox 144.0-1\n")[0].uninstall_command(),
            "sudo pacman -R -- firefox"
        );
        assert_eq!(
            parse("rpm", "kernel-1-2.x86_64\tkernel.x86_64\t1-2\n")[0].uninstall_command(),
            "sudo rpm -e -- kernel-1-2.x86_64"
        );
        let snaps = parse(
            "snap",
            "Name Version Rev Tracking Publisher Notes\nfirefox 144 1 stable mozilla -\n",
        );
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].uninstall_command(), "sudo snap remove -- firefox");
    }
}
