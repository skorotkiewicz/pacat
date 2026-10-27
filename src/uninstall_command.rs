use std::path::Path;
use std::process::{Command, Stdio};

use gio_unix::DesktopAppInfo;
use gtk4::gio;
use gtk4::glib::object::Cast;

pub(crate) fn for_app(app: &gio::AppInfo) -> Option<String> {
    let desktop = app.downcast_ref::<DesktopAppInfo>()?;
    if let Some(id) = desktop.string("X-Flatpak") {
        return Some(format!("flatpak uninstall -- {}", shell_quote(&id)));
    }
    if let Some(id) = desktop.string("X-SnapInstanceName") {
        return Some(format!("sudo snap remove -- {}", shell_quote(&id)));
    }
    let filename = desktop.filename()?;
    owner_command(&filename)
}

fn owner_command(filename: &Path) -> Option<String> {
    // Only package ownership queries are executed. Uninstall commands remain text.
    for (program, args, prefix) in [
        ("pacman", &["-Qoq", "--"][..], "sudo pacman -R --"),
        ("dpkg-query", &["-S", "--"][..], "sudo apt remove --"),
        (
            "rpm",
            &["-qf", "--queryformat", "%{NAME}", "--"][..],
            "sudo rpm -e --",
        ),
    ] {
        let Ok(output) = Command::new(program)
            .args(args)
            .arg(filename)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let output = String::from_utf8_lossy(&output.stdout);
        let mut packages = Vec::new();
        for line in output.lines() {
            let package = if program == "dpkg-query" {
                line.split_once(": ")?.0
            } else {
                line.trim()
            };
            if package.is_empty()
                || package.starts_with('-')
                || package.chars().any(char::is_control)
            {
                return None;
            }
            packages.push(shell_quote(package));
        }
        if !packages.is_empty() {
            return Some(format!("{prefix} {}", packages.join(" ")));
        }
    }
    None
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_+-./:@".contains(&byte))
    {
        value.into()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::shell_quote;

    #[test]
    fn quoted_identifiers_cannot_expand_shell_commands() {
        assert_eq!(shell_quote("firefox"), "firefox");
        assert_eq!(shell_quote("a'b;$(id)"), "'a'\\''b;$(id)'");
    }
}
