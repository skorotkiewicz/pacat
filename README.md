# pacat

A small Rust terminal app for browsing installed Linux applications and packages. Select a package to see its uninstall command. Pacat only prints uninstall commands; it never executes them.

```sh
cargo run --release
```

Type to filter by name, version or package manager. Search ignores case and matches all search words.

| Key | Action |
| --- | --- |
| Up / Down | Select a package |
| PageUp / PageDown | Move ten rows |
| Home / End | Jump to the first / last match |
| Enter | Exit and print the selected uninstall command |
| Backspace | Edit search |
| Esc | Clear search, or quit when search is empty |
| Ctrl+U | Clear search |
| Ctrl+C / Ctrl+Q | Quit |

Start with a search, or print a table for use in a terminal or script:

```sh
cargo run --release -- firefox
cargo run --release -- --list
cargo run --release -- --list firefox
```

Redirected output automatically uses table mode. Commands appear in the last column. Running `cargo install --path .` installs the `pacat` binary.

Pacat queries Pacman, dpkg, RPM, Flatpak and Snap when available. Native package lists include command-line tools, libraries and system packages. Flatpak lists applications with their exact ref and installation. AUR packages installed through Pacman are included. Manually unpacked apps, AppImages, language-specific package managers and apps in containers are outside this inventory. macOS and Windows are currently unsupported.

Discovery runs only fixed local inventory queries with no shell or elevated privileges. Uninstall commands are text, with shell-quoted identifiers. No uninstall, dependency cleanup or application-data deletion runs from this app. Failed inventory queries are reported on stderr while other sources remain available.

The command previews follow the [Pacman manual](https://man.archlinux.org/man/pacman.8), [Debian package query format](https://manpages.debian.org/unstable/dpkg/dpkg-query.1.en.html), [RPM manual](https://rpm.org/docs/latest/man/rpm.8), [Flatpak documentation](https://docs.flatpak.org/en/latest/using-flatpak.html), and [Snap documentation](https://snapcraft.io/docs/tutorials/get-started/).

```sh
just check
just test
```
