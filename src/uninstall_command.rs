use std::ffi::{OsStr, OsString};
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
        && let Some(command) = launch.steam_uninstall()
    {
        return Some(command);
    }
    if let Some(launch) = &launch
        && matches!(launch.executable.file_name()?.to_str()?, "wine" | "wine64")
    {
        // A missing prefix means the entry is dead: the uninstaller would only
        // create a fresh, empty prefix, so only the launcher remains to clean up.
        let default_prefix = glib::home_dir().join(".wine");
        let prefix = launch
            .wine_prefix
            .as_ref()
            .map(Path::new)
            .unwrap_or(default_prefix.as_path());
        if !prefix.is_dir()
            && let Some(filename) = desktop.filename()
        {
            return wine_dead_command(&filename);
        }
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
    let Some(executable) = glib::find_program_in_path(executable) else {
        // Dead entry: its executable is gone, so only the launcher remains to clean up.
        return trash_command(&[desktop.filename()?]);
    };
    lookup_path(&executable, &mut owner).or_else(|| {
        let resolved = executable.canonicalize().ok()?;
        let desktop_file = desktop.filename()?;
        bundle_trash_command(&resolved, &desktop_file)
    })
}

// A dead wine entry leaves only its launchers behind. Wine gives every app
// its own directory under wine/Programs, so the whole directory is the
// cleanup target unless another launcher shares it.
fn wine_dead_command(desktop_file: &Path) -> Option<String> {
    let programs = glib::home_dir().join(".local/share/applications/wine/Programs");
    dead_wine_launcher(desktop_file, &programs)
}

fn dead_wine_launcher(desktop_file: &Path, programs: &Path) -> Option<String> {
    let dir = desktop_file.parent()?;
    let shared = dir.read_dir().ok()?.filter_map(Result::ok).any(|entry| {
        let path = entry.path();
        path.extension() == Some(OsStr::new("desktop")) && path != desktop_file
    });
    if dir.starts_with(programs) && !shared {
        trash_command(&[dir.to_path_buf()])
    } else {
        trash_command(&[desktop_file.to_path_buf()])
    }
}

fn trash_command(paths: &[PathBuf]) -> Option<String> {
    let paths = paths
        .iter()
        .map(|path| path.to_str().map(shell_quote))
        .collect::<Option<Vec<_>>>()?;
    Some(format!("gio trash -- {}", paths.join(" ")))
}

// Detect self-contained bundles such as ~/apps/Delta.app/bin/delta and suggest
// trashing the whole bundle together with its launcher.
fn bundle_trash_command(executable: &Path, desktop_file: &Path) -> Option<String> {
    // A self-contained AppImage is the whole application; remove it as a file.
    if executable.extension() == Some(OsStr::new("AppImage")) {
        return trash_command(&[executable.to_path_buf(), desktop_file.to_path_buf()]);
    }
    let bin = executable.parent()?;
    if bin.file_name()? != "bin" {
        return None;
    }
    let bundle = bin.parent()?;
    // A bin/ under a standard prefix is a shared system location, not a bundle.
    let prefixes = [
        glib::home_dir().join(".local"),
        PathBuf::from("/usr"),
        PathBuf::from("/usr/local"),
        PathBuf::from("/opt"),
    ];
    if prefixes.iter().any(|prefix| bundle == prefix.as_path()) || !bundle.is_dir() {
        return None;
    }
    let mut paths = vec![bundle.to_path_buf(), desktop_file.to_path_buf()];
    // A launcher symlink in ~/.local/bin pointing into the bundle goes with it.
    let link = glib::home_dir()
        .join(".local/bin")
        .join(executable.file_name()?);
    if link.is_symlink() && link.canonicalize().ok() == Some(executable.to_path_buf()) {
        paths.push(link);
    }
    trash_command(&paths)
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
    arguments: Vec<OsString>,
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
            arguments: args[index + 1..].to_vec(),
        })
    }

    fn steam_uninstall(&self) -> Option<String> {
        if self.executable.file_name()? != "steam" {
            return None;
        }
        let id = self
            .arguments
            .iter()
            .find_map(|argument| {
                let argument = argument.to_str()?;
                argument
                    .strip_prefix("steam://rungameid/")
                    .or_else(|| argument.strip_prefix("steam://run/"))
                    .map(|suffix| suffix.split('/').next().unwrap_or_default())
            })
            .or_else(|| {
                self.arguments
                    .windows(2)
                    .find(|args| args[0] == "-applaunch")
                    .and_then(|args| args[1].to_str())
            })?;
        // Steam shortcut IDs can exceed u32. They are not uninstallable library app IDs.
        let id = id.parse::<u32>().ok().filter(|id| *id > 0)?;
        Some(format!(
            "{} steam://uninstall/{id}",
            shell_quote(self.executable.to_str()?)
        ))
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
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use gio_unix::DesktopAppInfo;
    use gtk4::glib::{KeyFile, object::Cast};

    use super::{Launch, dead_wine_launcher, for_app_with_owner, shell_quote};

    struct TestBundle {
        root: PathBuf,
        executable: PathBuf,
        launcher: PathBuf,
        bundled_launcher: PathBuf,
    }

    impl TestBundle {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "pacat-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let bundle = root.join("Example App.app");
            let executable = bundle.join("bin/example");
            let launcher = root.join("applications/org.example.App.desktop");
            let bundled_launcher = bundle.join("share/applications/org.example.App.desktop");
            for file in [&executable, &launcher, &bundled_launcher] {
                fs::create_dir_all(file.parent().expect("fixture parent"))
                    .expect("create fixture directory");
            }
            let true_executable =
                gtk4::glib::find_program_in_path("true").expect("system true executable");
            fs::copy(true_executable, &executable).expect("create fixture executable");
            let contents = format!(
                "[Desktop Entry]\nType=Application\nName=Example App\nExec=\"{}\" %U\n",
                executable.display()
            );
            fs::write(&launcher, &contents).expect("create launcher");
            fs::write(&bundled_launcher, contents).expect("create bundled launcher");
            Self {
                root,
                executable,
                launcher,
                bundled_launcher,
            }
        }

        fn app(&self) -> DesktopAppInfo {
            DesktopAppInfo::from_filename(&self.launcher).expect("valid launcher")
        }
    }

    impl Drop for TestBundle {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn manual_bundles_are_detected_without_application_names_or_fixed_paths() {
        let bundle = TestBundle::new();
        let app = bundle.app();
        let command = for_app_with_owner(app.upcast_ref(), |_| None);
        assert_eq!(
            command,
            Some(format!(
                "gio trash -- {} {}",
                shell_quote(
                    bundle
                        .executable
                        .parent()
                        .and_then(Path::parent)
                        .expect("bundle root")
                        .to_str()
                        .expect("fixture path")
                ),
                shell_quote(bundle.launcher.to_str().expect("fixture path"))
            ))
        );
    }

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
    fn wine_dead_command_targets_the_app_directory() {
        let root = std::env::temp_dir().join(format!(
            "pacat-wine-test-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        // The launcher must sit under wine/Programs, so test the helper with
        // a fixture root instead of the real home directory.
        let programs = root.join("wine/Programs");
        let dir = programs.join("YootTower");
        fs::create_dir_all(&dir).expect("create wine programs directory");
        let launcher = dir.join("Yoot Tower.desktop");
        fs::write(
            &launcher,
            "[Desktop Entry]\nType=Application\nName=Yoot Tower\n",
        )
        .expect("create launcher");
        let command = dead_wine_launcher(&launcher, &programs);
        let expected = format!(
            "gio trash -- {}",
            shell_quote(dir.to_str().expect("fixture path"))
        );
        assert_eq!(command.as_deref(), Some(expected.as_str()));

        // A launcher outside wine/Programs only loses its own file.
        let command = dead_wine_launcher(&root.join("plain.desktop"), &programs);
        let expected = format!(
            "gio trash -- {}",
            shell_quote(root.join("plain.desktop").to_str().expect("fixture path"))
        );
        assert_eq!(command.as_deref(), Some(expected.as_str()));
        fs::remove_dir_all(&root).expect("clean up fixture");
    }

    #[test]
    fn appimages_are_trashed_as_single_files_with_their_launcher() {
        let root = std::env::temp_dir().join(format!(
            "pacat-appimage-test-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        let appimage = root.join("solidcraft-0.7.0-linux-x86_64.AppImage");
        fs::create_dir_all(&root).expect("create fixture directory");
        fs::write(&appimage, "#!/bin/sh\n").expect("create fixture AppImage");
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&appimage, fs::Permissions::from_mode(0o755))
            .expect("make fixture executable");
        let exec = format!("\"{}\" %F", appimage.display());
        let launcher = root.join("ai.storyteller.solvecraft.desktop");
        fs::write(
            &launcher,
            format!("[Desktop Entry]\nType=Application\nName=SolidCraft\nExec={exec}\n"),
        )
        .expect("create fixture launcher");
        let app = DesktopAppInfo::from_filename(&launcher).expect("valid launcher");
        let command = for_app_with_owner(app.upcast_ref(), |_| None);
        let expected = format!(
            "gio trash -- {} {}",
            shell_quote(appimage.to_str().expect("fixture path")),
            shell_quote(launcher.to_str().expect("fixture path"))
        );
        assert_eq!(command.as_deref(), Some(expected.as_str()));
        fs::remove_dir_all(&root).expect("clean up fixture");
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
    fn steam_games_use_the_game_id_instead_of_uninstalling_steam() {
        let app = desktop("env steam steam://rungameid/730");
        assert_eq!(
            for_app_with_owner(app.upcast_ref(), |_| None).as_deref(),
            Some("steam steam://uninstall/730")
        );
    }

    #[test]
    fn steam_handles_applaunch_and_rejects_shortcuts_and_invalid_ids() {
        let launch =
            Launch::parse(Path::new("steam -applaunch 730 -novid")).expect("valid command");
        assert_eq!(
            launch.steam_uninstall().as_deref(),
            Some("steam steam://uninstall/730")
        );
        for command in [
            "steam steam://rungameid/12345678901234567890",
            "steam steam://rungameid/0",
            "steam steam://rungameid/730;bad",
            "steam steam://rungameid/",
            "steam",
        ] {
            let launch = Launch::parse(Path::new(command)).expect("valid command text");
            assert!(
                launch.steam_uninstall().is_none(),
                "invalid game ID accepted: {command}"
            );
        }
    }

    #[test]
    fn quoted_identifiers_cannot_expand_shell_commands() {
        assert_eq!(shell_quote("firefox"), "firefox");
        assert_eq!(shell_quote("a'b;$(id)"), "'a'\\''b;$(id)'");
    }
}
