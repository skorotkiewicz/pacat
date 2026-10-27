# pacat

A small Rust and GTK application that lists installed desktop applications and shows their uninstall commands.

```sh
cargo run
```

The list uses the same discovery as Rudo's application picker: `gio::AppInfo::all()`, filtered with `should_show()` and sorted by display name. Rows show the application's icon, name and desktop ID. Search filters by name, ID or executable.

Select an application to see its uninstall command. The Copy command button copies the text to your clipboard. Pacat never executes uninstall commands.

Pacman, Debian and RPM package ownership is detected using read-only queries on the desktop file. Flatpak and Snap applications are recognized through desktop-entry metadata. Applications without a known package owner stay in the list and show that an uninstall command could not be found.

Requires Linux, GTK 4 development libraries, and Rust.

```sh
just check
just test
```
