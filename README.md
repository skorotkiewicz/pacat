# pacat

A small Rust and GTK application that lists installed desktop applications and CLI commands and shows their uninstall commands.

```sh
cargo run
```

The **Desktop apps** tab shows every installed desktop entry that is visible in application menus (`gio::AppInfo::all()`, filtered with `should_show()`), sorted by display name. Rows show the application's icon, name, description and desktop ID, and the detail pane shows the launcher's file path. Search filters by name, ID, executable or description.

The **CLI apps** tab lists executable files in Pacat's `PATH`, including tools such as `opencode` when available there. Commands are sorted by name, with duplicate names showing only the first executable in `PATH` order. Symlinks are included when their targets are executable files. This list also includes helper commands and GUI launchers. Shell aliases, functions and commands outside Pacat's `PATH` are not included. Search matches command names and paths. Discovery never runs the commands.

Each tab has a refresh button to reload its list after installs and removals.

Select an application to see its uninstall command. The Copy command button copies the text to your clipboard. Pacat never executes uninstall commands.

What each application gets:

- **Pacman, Debian and RPM packages** are detected with read-only ownership queries on the desktop file, the executable and its resolved symlink target.
- **Flatpak and Snap** applications are recognized through desktop-entry metadata.
- **Steam games** show a `steam://uninstall/APPID` command for Steam's uninstall dialog.
- **Wine applications** show `wine uninstaller` with the launcher's `WINEPREFIX`. When the shortcut resolves, a second `gio trash` command removes the program files and launcher together. If the prefix no longer exists, the command instead trashes the application's own directory under `wine/Programs/`.
- **AppImages** are trashed as the single self-contained file plus the launcher.
- **Bundled applications** (an executable under `<bundle>/bin/`) trash the whole bundle, launcher and any matching `~/.local/bin` symlink.

All suggested paths are shell-quoted and commands are displayed only. Applications without a known uninstall command stay in the list and show that the command could not be determined.

For CLI commands, uninstall previews use system package ownership queries on the executable and its resolved symlink target. Commands installed through other installers or copied manually remain visible even when their uninstall command cannot be determined.

Requires Linux, GTK 4 development libraries, and Rust.

```sh
just check
just test
```
