use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use gio_unix::DesktopAppInfo;
use gtk4::gio;
use gtk4::gio::prelude::AppInfoExt;
use gtk4::glib::{self, object::Cast};

pub(crate) fn for_app(app: &gio::AppInfo) -> Option<String> {
    for_app_with_owner(app, owner_command)
}

fn for_app_with_owner(
    app: &gio::AppInfo,
    mut owner: impl FnMut(&Path) -> Option<String>,
) -> Option<String> {
    let desktop = app.downcast_ref::<DesktopAppInfo>()?;
    if let Some(id) = desktop.string("X-Flatpak") {
        return Some(format!("flatpak uninstall -- {}", shell_quote(&id)));
    }
    if let Some(id) = desktop.string("X-SnapInstanceName") {
        return Some(format!("sudo snap remove -- {}", shell_quote(&id)));
    }
    let launch = app
        .commandline()
        .and_then(|command| Launch::parse(&command));
    if let Some(launch) = &launch
        && matches!(launch.executable.file_name()?.to_str()?, "wine" | "wine64")
    {
        let wine = shell_quote(launch.executable.to_str()?);
        return Some(match &launch.wine_prefix {
            Some(prefix) => format!(
                "WINEPREFIX={} {wine} uninstaller",
                shell_quote(prefix.to_str()?)
            ),
            None => format!("{wine} uninstaller"),
        });
    }
    if let Some(filename) = desktop.filename()
        && let Some(command) = lookup_path(&filename, &mut owner)
    {
        return Some(command);
    }
    let executable = launch?.executable;
    // A shared interpreter or launcher does not identify the application's package.
    if matches!(
        executable.file_name()?.to_str()?,
        "env"
            | "sh"
            | "bash"
            | "dash"
            | "zsh"
            | "fish"
            | "python"
            | "python3"
            | "node"
            | "perl"
            | "ruby"
            | "steam"
            | "flatpak"
            | "snap"
    ) {
        return None;
    }
    let executable = glib::find_program_in_path(executable)?;
    lookup_path(&executable, &mut owner)
}

fn lookup_path(path: &Path, owner: &mut impl FnMut(&Path) -> Option<String>) -> Option<String> {
    owner(path).or_else(|| {
        let resolved = path.canonicalize().ok()?;
        (resolved != path).then(|| owner(&resolved)).flatten()
    })
}

struct Launch {
    executable: PathBuf,
    wine_prefix: Option<OsString>,
}

impl Launch {
    fn parse(command: &Path) -> Option<Self> {
        // Parse text only. Never invoke the desktop entry or its shell commands.
        let args = glib::shell_parse_argv(command.as_os_str()).ok()?;
        let mut index = 0;
        let mut wine_prefix = None;
        if Path::new(args.first()?).file_name()? == "env" {
            index = 1;
            while let Some(argument) = args.get(index) {
                let argument = argument.to_str()?;
                match argument {
                    "--" => {
                        index += 1;
                        break;
                    }
                    "-i" | "--ignore-environment" => {}
                    "-u" | "--unset" => {
                        args.get(index + 1)?;
                        index += 1;
                    }
                    value if value.starts_with("--unset=") => {}
                    value if value.contains('=') && !value.starts_with('-') => {
                        if let Some(prefix) = value.strip_prefix("WINEPREFIX=") {
                            wine_prefix = Some(OsString::from(prefix));
                        }
                    }
                    value if value.starts_with('-') => return None,
                    _ => break,
                }
                index += 1;
            }
        }
        Some(Self {
            executable: PathBuf::from(args.get(index)?),
            wine_prefix,
        })
    }
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
    use std::path::Path;

    use gio_unix::DesktopAppInfo;
    use gtk4::glib::{KeyFile, object::Cast};

    use super::{for_app_with_owner, shell_quote};

    fn desktop(exec: &str) -> DesktopAppInfo {
        let keyfile = KeyFile::new();
        keyfile.set_string("Desktop Entry", "Type", "Application");
        keyfile.set_string("Desktop Entry", "Name", "Test application");
        keyfile.set_string("Desktop Entry", "Exec", exec);
        DesktopAppInfo::from_keyfile(&keyfile).expect("valid desktop entry")
    }

    fn true_owner(path: &Path) -> Option<String> {
        (path.file_name().is_some_and(|name| name == "true"))
            .then(|| "sudo pacman -R -- test-app".into())
    }

    #[test]
    fn executable_ownership_resolves_an_unowned_launcher() {
        let app = desktop("/usr/bin/true %F");
        assert_eq!(
            for_app_with_owner(app.upcast_ref(), true_owner).as_deref(),
            Some("sudo pacman -R -- test-app")
        );
    }

    #[test]
    fn env_wrapper_resolves_the_application_instead_of_env() {
        let app = desktop("env GDK_BACKEND=x11 true %U");
        assert_eq!(
            for_app_with_owner(app.upcast_ref(), true_owner).as_deref(),
            Some("sudo pacman -R -- test-app")
        );
    }

    #[test]
    fn wine_apps_show_the_uninstaller_for_their_own_prefix() {
        let app = desktop("env WINEPREFIX=\"/tmp/game prefix\" wine game.exe");
        let command = for_app_with_owner(app.upcast_ref(), |_| {
            panic!("Wine games must not query the Wine package")
        });
        assert_eq!(
            command.as_deref(),
            Some("WINEPREFIX='/tmp/game prefix' wine uninstaller")
        );
    }

    #[test]
    fn shared_shell_is_not_suggested_as_the_application_to_remove() {
        let app = desktop("sh -c /tmp/custom-application");
        let command = for_app_with_owner(app.upcast_ref(), |_| {
            panic!("must not suggest uninstalling a shared shell")
        });
        assert!(
            command.is_none(),
            "unknown apps must not uninstall their interpreter"
        );
    }

    #[test]
    fn quoted_identifiers_cannot_expand_shell_commands() {
        assert_eq!(shell_quote("firefox"), "firefox");
        assert_eq!(shell_quote("a'b;$(id)"), "'a'\\''b;$(id)'");
    }
}
