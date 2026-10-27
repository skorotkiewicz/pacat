use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Pacman,
    Debian,
    Rpm,
    Snap,
    Flatpak { installation: String },
}

impl fmt::Display for Source {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pacman => formatter.write_str("pacman"),
            Self::Debian => formatter.write_str("dpkg"),
            Self::Rpm => formatter.write_str("rpm"),
            Self::Snap => formatter.write_str("snap"),
            Self::Flatpak { installation } => write!(formatter, "flatpak:{installation}"),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Package {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) source: Source,
}

impl Package {
    pub(crate) fn new(id: &str, name: &str, version: &str, source: Source) -> Option<Self> {
        if id.is_empty() || id.starts_with('-') || id.chars().any(char::is_control) {
            return None;
        }
        Some(Self {
            id: id.to_owned(),
            name: clean(name),
            version: clean(version),
            source,
        })
    }

    // This returns text only. Package data never becomes a subprocess invocation.
    pub(crate) fn uninstall_command(&self) -> String {
        let id = shell_quote(&self.id);
        match &self.source {
            Source::Pacman => format!("sudo pacman -R -- {id}"),
            Source::Debian => format!("sudo apt remove -- {id}"),
            Source::Rpm => format!("sudo rpm -e -- {id}"),
            Source::Snap => format!("sudo snap remove -- {id}"),
            Source::Flatpak { installation } => {
                let scope = if installation == "user" {
                    "--user".to_owned()
                } else {
                    shell_quote(&format!("--installation={installation}"))
                };
                format!("flatpak uninstall {scope} -- {id}")
            }
        }
    }

    pub(crate) fn matches(&self, query: &str) -> bool {
        let searchable =
            format!("{} {} {} {}", self.name, self.id, self.version, self.source).to_lowercase();
        query
            .split_whitespace()
            .all(|word| searchable.contains(&word.to_lowercase()))
    }
}

fn clean(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_+-./:@=".contains(&byte))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::{Package, Source, shell_quote};

    #[test]
    fn shell_metacharacters_remain_literal() {
        assert_eq!(
            shell_quote("a'b;$(touch /tmp/nope)"),
            "'a'\\''b;$(touch /tmp/nope)'"
        );
        assert_eq!(
            shell_quote("org.example.App/x86_64/stable"),
            "org.example.App/x86_64/stable"
        );
    }

    #[test]
    fn flatpak_commands_target_the_exact_installation_and_ref() {
        for (installation, scope) in [
            ("user", "--user"),
            ("system", "--installation=system"),
            ("extra", "--installation=extra"),
        ] {
            let package = Package::new(
                "app/org.example.App/x86_64/beta",
                "Example",
                "1",
                Source::Flatpak {
                    installation: installation.into(),
                },
            )
            .expect("valid package");
            assert_eq!(
                package.uninstall_command(),
                format!("flatpak uninstall {scope} -- app/org.example.App/x86_64/beta")
            );
        }
    }

    #[test]
    fn identifiers_cannot_inject_terminal_controls_or_options() {
        for id in ["", "--all", "app\x1b[2J", "app\nother"] {
            assert!(
                Package::new(id, "name", "1", Source::Pacman).is_none(),
                "accepted invalid identifier {id:?}"
            );
        }
    }
}
