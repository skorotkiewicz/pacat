use gio::prelude::{AppInfoExt, ApplicationExt, ApplicationExtManual};
use gio_unix::DesktopAppInfo;
use gtk::glib::object::{Cast, ObjectExt};
use gtk::prelude::{BoxExt, ButtonExt, EditableExt, GtkWindowExt, ListBoxRowExt, WidgetExt};
use gtk::{gio, glib};
use gtk4 as gtk;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::cli_group::CliGroup;
use crate::uninstall_command;

pub(crate) fn main() -> glib::ExitCode {
    let application = gtk::Application::builder()
        .application_id("io.github.pacat")
        .build();
    application.connect_activate(build_window);
    application.run()
}

#[derive(Clone, Copy)]
enum AppKind {
    Desktop,
    Cli,
}

impl AppKind {
    async fn load(self) -> Option<Vec<InstalledApp>> {
        match self {
            Self::Desktop => {
                let mut apps: Vec<_> = gio::AppInfo::all()
                    .into_iter()
                    .filter(|app| app.should_show() && app.id().is_some())
                    .map(InstalledApp::Desktop)
                    .collect();
                apps.sort_by_cached_key(|app| app.name().to_lowercase());
                Some(apps)
            }
            Self::Cli => {
                let desktop_executables = gio::AppInfo::all()
                    .into_iter()
                    .filter(|app| app.should_show() && app.id().is_some())
                    .filter_map(|app| uninstall_command::desktop_executable(&app))
                    .collect::<Vec<_>>();
                let groups = gio::spawn_blocking(move || CliGroup::installed(&desktop_executables))
                    .await
                    .ok()?;
                Some(groups.into_iter().map(InstalledApp::Cli).collect())
            }
        }
    }

    fn description(self, apps: &[InstalledApp]) -> String {
        let count = apps.len();
        match self {
            Self::Desktop => {
                format!("{count} applications · Select an app to see its uninstall command")
            }
            Self::Cli => {
                let commands: usize = apps
                    .iter()
                    .map(|app| match app {
                        InstalledApp::Cli(group) => group.binaries.len(),
                        InstalledApp::Desktop(_) => 0,
                    })
                    .sum();
                format!(
                    "{commands} commands in {count} groups · Desktop app executables are hidden"
                )
            }
        }
    }
}

enum InstalledApp {
    Desktop(gio::AppInfo),
    Cli(CliGroup),
}

impl InstalledApp {
    fn name(&self) -> String {
        match self {
            Self::Desktop(app) => app.display_name().into(),
            Self::Cli(app) => app.name.clone(),
        }
    }

    fn identifier(&self) -> String {
        match self {
            Self::Desktop(app) => app.id().unwrap_or_default().into(),
            Self::Cli(group) => group
                .uninstall_command
                .clone()
                .unwrap_or_else(|| "No system package owner found".into()),
        }
    }

    fn summary(&self) -> Option<String> {
        match self {
            Self::Desktop(app) => app
                .downcast_ref::<DesktopAppInfo>()
                .and_then(|entry| entry.description().or_else(|| entry.generic_name()))
                .map(Into::into),
            Self::Cli(group) => Some(
                group
                    .binaries
                    .iter()
                    .map(|app| app.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        }
    }

    fn icon(&self) -> Option<gio::Icon> {
        match self {
            Self::Desktop(app) => app.icon(),
            Self::Cli(_) => None,
        }
    }

    fn file(&self) -> Option<PathBuf> {
        match self {
            Self::Desktop(app) => app
                .downcast_ref::<DesktopAppInfo>()
                .and_then(|entry| entry.filename()),
            Self::Cli(_) => None,
        }
    }

    fn search_key(&self) -> String {
        let executable = match self {
            Self::Desktop(app) => app.executable().display().to_string(),
            Self::Cli(group) => group
                .binaries
                .iter()
                .map(|app| app.executable.display().to_string())
                .collect::<Vec<_>>()
                .join(" "),
        };
        format!(
            "{} {} {} {}",
            self.name(),
            self.identifier(),
            executable,
            self.summary().unwrap_or_default()
        )
        .to_lowercase()
    }

    fn uninstall_command(&self) -> Option<String> {
        match self {
            Self::Desktop(app) => uninstall_command::for_app(app),
            Self::Cli(group) => group.uninstall_command.clone(),
        }
    }
}

// Rc is for shared UI state between the selection, filter and refresh
// callbacks; GTK widgets outlive single borrows.
type Shared<T> = Rc<RefCell<T>>;

// Drop all rows and rebuild the list from `apps`.
fn rebuild_list(
    list: &gtk::ListBox,
    apps: &[InstalledApp],
    keys: &Shared<Vec<String>>,
    description: &gtk::Label,
    kind: AppKind,
) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
    description.set_text(&kind.description(apps));
    *keys.borrow_mut() = apps.iter().map(InstalledApp::search_key).collect();
    for app in apps {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.set_margin_top(10);
        row.set_margin_bottom(10);
        row.set_margin_start(12);
        row.set_margin_end(12);
        let image = gtk::Image::new();
        image.set_pixel_size(36);
        if let Some(icon) = app.icon() {
            image.set_from_gicon(&icon);
        } else {
            image.set_icon_name(Some("application-x-executable"));
        }
        row.append(&image);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
        let name = gtk::Label::new(Some(&app.name()));
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.add_css_class("heading");
        let id = gtk::Label::new(Some(&app.identifier()));
        id.set_xalign(0.0);
        id.set_ellipsize(gtk::pango::EllipsizeMode::End);
        id.add_css_class("dim-label");
        let summary = app.summary();
        text.set_hexpand(true);
        text.append(&name);
        if let Some(summary) = summary {
            let summary = gtk::Label::new(Some(&summary));
            summary.set_xalign(0.0);
            summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
            summary.add_css_class("dim-label");
            text.append(&summary);
        }
        text.append(&id);
        if let InstalledApp::Cli(group) = app {
            let binaries = gtk::Box::new(gtk::Orientation::Vertical, 4);
            for binary in &group.binaries {
                let label = gtk::Label::new(Some(&format!(
                    "{}\n{}",
                    binary.name,
                    binary.executable.display()
                )));
                label.set_xalign(0.0);
                label.set_selectable(true);
                label.set_wrap(true);
                label.add_css_class("monospace");
                label.set_margin_top(4);
                binaries.append(&label);
            }
            let expander = gtk::Expander::new(Some(&format!("{} binaries", group.binaries.len())));
            expander.set_child(Some(&binaries));
            text.append(&expander);
        }
        row.append(&text);
        list.append(&row);
    }
}

fn build_window(application: &gtk::Application) {
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("Pacat")
        .default_width(620)
        .default_height(720)
        .build();
    let tabs = gtk::Notebook::new();
    tabs.append_page(
        &build_page(AppKind::Desktop),
        Some(&gtk::Label::new(Some("Desktop apps"))),
    );
    tabs.append_page(
        &build_page(AppKind::Cli),
        Some(&gtk::Label::new(Some("CLI apps"))),
    );
    window.set_child(Some(&tabs));
    window.present();
}

fn build_page(kind: AppKind) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);

    let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title = gtk::Label::new(Some(match kind {
        AppKind::Desktop => "Installed desktop applications",
        AppKind::Cli => "Installed CLI commands",
    }));
    title.set_xalign(0.0);
    title.add_css_class("title-2");
    title.set_hexpand(true);
    title_row.append(&title);
    let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
    refresh.set_tooltip_text(Some("Reload the application list"));
    title_row.append(&refresh);
    content.append(&title_row);
    let description = gtk::Label::new(Some("Loading…"));
    description.set_xalign(0.0);
    description.set_wrap(true);
    description.add_css_class("dim-label");
    content.append(&description);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(match kind {
        AppKind::Desktop => "Search applications",
        AppKind::Cli => "Search packages, commands or paths",
    }));
    content.append(&search);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.add_css_class("boxed-list");
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    content.append(&scroll);

    let heading = gtk::Label::new(Some("Uninstall command"));
    heading.set_xalign(0.0);
    heading.add_css_class("heading");
    content.append(&heading);
    let command = gtk::Label::new(Some("Select an application"));
    command.set_selectable(true);
    command.set_wrap(true);
    command.set_xalign(0.0);
    command.add_css_class("monospace");
    command.set_margin_top(6);
    command.set_margin_bottom(6);
    content.append(&command);
    let file = gtk::Label::new(None);
    file.set_selectable(true);
    file.set_wrap(true);
    file.set_xalign(0.0);
    file.add_css_class("dim-label");
    file.set_visible(false);
    content.append(&file);
    let copy = gtk::Button::with_label("Copy command");
    copy.set_sensitive(false);
    content.append(&copy);
    let notice = gtk::Label::new(Some(
        "Commands are displayed only. Nothing is uninstalled by this app.",
    ));
    notice.set_wrap(true);
    notice.add_css_class("dim-label");
    content.append(&notice);

    // Shared between the selection, filter and refresh callbacks.
    let apps: Shared<Vec<InstalledApp>> = Rc::new(RefCell::new(Vec::new()));
    let keys: Shared<Vec<String>> = Rc::new(RefCell::new(Vec::new()));
    let filter_search = search.clone();
    let filter_keys = keys.clone();
    list.set_filter_func(move |row| {
        let query = filter_search.text().to_lowercase();
        filter_keys
            .borrow()
            .get(row.index() as usize)
            .is_some_and(|key| key.contains(query.trim()))
    });
    let weak_list = list.downgrade();
    let search_command = command.clone();
    let search_copy = copy.clone();
    let search_file = file.clone();
    search.connect_search_changed(move |_| {
        if let Some(list) = weak_list.upgrade() {
            list.unselect_all();
            list.invalidate_filter();
        }
        search_command.set_text("Select an application");
        search_copy.set_sensitive(false);
        search_file.set_visible(false);
    });
    let selected_command = command.clone();
    let selected_copy = copy.clone();
    let selected_apps = apps.clone();
    let selected_file = file.clone();
    list.connect_row_selected(move |_, row| {
        let apps = selected_apps.borrow();
        let Some(app) =
            row.and_then(|row| apps.get(row.index() as usize))
        else {
            selected_command.set_text("Select an application");
            selected_copy.set_sensitive(false);
            selected_file.set_visible(false);
            return;
        };
        selected_file.set_visible(false);
        if let Some(path) = app.file() {
            selected_file.set_text(&path.display().to_string());
            selected_file.set_visible(true);
        }
        match app.uninstall_command() {
            Some(text) => {
                selected_command.set_text(&text);
                selected_copy.set_sensitive(true);
            }
            None => {
                selected_command.set_text(match app {
                    InstalledApp::Desktop(_) => "Could not determine an uninstall command for this application.",
                    InstalledApp::Cli(_) => "No system package owner found. This command may use another installer or have been installed manually.",
                });
                selected_copy.set_sensitive(false);
            }
        }
    });
    let refresh_list = list.clone();
    let refresh_apps = apps.clone();
    let refresh_keys = keys.clone();
    let refresh_description = description.clone();
    let refresh_command = command.clone();
    let refresh_copy = copy.clone();
    let refresh_file = file.clone();
    copy.connect_clicked(move |_| command.clipboard().set_text(&command.text()));
    refresh.connect_clicked(move |refresh| {
        refresh.set_sensitive(false);
        refresh_description.set_text("Loading…");
        let refresh = refresh.clone();
        let list = refresh_list.clone();
        let apps = refresh_apps.clone();
        let keys = refresh_keys.clone();
        let description = refresh_description.clone();
        let command = refresh_command.clone();
        let copy = refresh_copy.clone();
        let file = refresh_file.clone();
        glib::MainContext::default().spawn_local(async move {
            match kind.load().await {
                Some(loaded) => {
                    list.unselect_all();
                    rebuild_list(&list, &loaded, &keys, &description, kind);
                    *apps.borrow_mut() = loaded;
                    command.set_text("Select an application");
                    copy.set_sensitive(false);
                    file.set_visible(false);
                }
                None => description.set_text("Could not load CLI commands. Try refreshing."),
            }
            refresh.set_sensitive(true);
        });
    });
    refresh.emit_clicked();
    content
}
