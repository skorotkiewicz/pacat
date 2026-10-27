# pacat

A small Rust and GTK application that lists installed desktop applications and shows their uninstall commands.

```sh
cargo run
```

The list uses the same discovery as Rudo's application picker: `gio::AppInfo::all()`, filtered with `should_show()` and sorted by display name. Rows show the application's icon, name and desktop ID. Search filters by name, ID or executable.

Select an application to see its uninstall command. The Copy command button copies the text to your clipboard. Pacat never executes uninstall commands.

Pacman, Debian and RPM package ownership is detected using read-only queries on the desktop file, then the application's executable and resolved symlink target. Flatpak and Snap applications are recognized through desktop-entry metadata. Wine applications show `wine uninstaller` with the launcher's `WINEPREFIX`, so you can select the Windows application to remove. Applications without a known uninstall command stay in the list and show that the command could not be determined.

Requires Linux, GTK 4 development libraries, and Rust.

Steam games show a `steam://uninstall/APPID` command for Steam's uninstall dialog. These commands are displayed only.

```sh
just check
just test
```
