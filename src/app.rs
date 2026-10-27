use gio::prelude::{AppInfoExt, ApplicationExt, ApplicationExtManual};
use gio_unix::DesktopAppInfo;
use gtk::glib::object::{Cast, ObjectExt};
use gtk::prelude::{BoxExt, ButtonExt, EditableExt, GtkWindowExt, ListBoxRowExt, WidgetExt};
use gtk::{gio, glib};
use gtk4 as gtk;

use std::cell::RefCell;
use std::rc::Rc;

use crate::uninstall_command;

pub(crate) fn main() -> glib::ExitCode {
    let application = gtk::Application::builder()
        .application_id("io.github.pacat")
        .build();
    application.connect_activate(build_window);
    application.run()
}

// Visible desktop apps, sorted by display name.
fn load_apps() -> Vec<gio::AppInfo> {
    let mut apps: Vec<_> = gio::AppInfo::all()
        .into_iter()
        .filter(|app| app.should_show() && app.id().is_some())
        .collect();
    apps.sort_by_cached_key(|app| app.display_name().to_lowercase());
    apps
}

// Everything the search box matches, lowercased.
fn search_key(app: &gio::AppInfo) -> String {
    let description = app
        .downcast_ref::<DesktopAppInfo>()
        .and_then(|entry| entry.description().or_else(|| entry.generic_name()));
    format!(
        "{} {} {} {}",
        app.display_name(),
        app.id().unwrap_or_default(),
        app.executable().display(),
        description.unwrap_or_default()
    )
    .to_lowercase()
}

// Rc is for shared UI state between the selection, filter and refresh
// callbacks; GTK widgets outlive single borrows.
type Shared<T> = Rc<RefCell<T>>;

// Drop all rows and rebuild the list from `apps`.
fn rebuild_list(
    list: &gtk::ListBox,
    apps: &[gio::AppInfo],
    keys: &Shared<Vec<String>>,
    description: &gtk::Label,
) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
    description.set_text(&format!(
        "{} applications · Select an app to see its uninstall command",
        apps.len()
    ));
    *keys.borrow_mut() = apps.iter().map(search_key).collect();
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
        let name = gtk::Label::new(Some(&app.display_name()));
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.add_css_class("heading");
        let id = gtk::Label::new(app.id().as_deref());
        id.set_xalign(0.0);
        id.set_ellipsize(gtk::pango::EllipsizeMode::End);
        id.add_css_class("dim-label");
        let entry = app.downcast_ref::<DesktopAppInfo>();
        let summary = entry.and_then(|entry| entry.description().or_else(|| entry.generic_name()));
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
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);

    let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let title = gtk::Label::new(Some("Installed applications"));
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
    search.set_placeholder_text(Some("Search applications"));
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
    let apps = Rc::new(RefCell::new(Vec::new()));
    let keys = Rc::new(RefCell::new(Vec::new()));
    let initial = load_apps();
    rebuild_list(&list, &initial, &keys, &description);
    *apps.borrow_mut() = initial;
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
        let Some(app) =
            row.and_then(|row| selected_apps.borrow().get(row.index() as usize).cloned())
        else {
            selected_command.set_text("Select an application");
            selected_copy.set_sensitive(false);
            selected_file.set_visible(false);
            return;
        };
        if let Some(path) = app
            .downcast_ref::<DesktopAppInfo>()
            .and_then(|entry| entry.filename())
        {
            selected_file.set_text(&path.display().to_string());
            selected_file.set_visible(true);
        }
        match uninstall_command::for_app(&app) {
            Some(text) => {
                selected_command.set_text(&text);
                selected_copy.set_sensitive(true);
            }
            None => {
                selected_command
                    .set_text("Could not determine an uninstall command for this application.");
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
    refresh.connect_clicked(move |_| {
        let apps = load_apps();
        refresh_list.unselect_all();
        rebuild_list(&refresh_list, &apps, &refresh_keys, &refresh_description);
        *refresh_apps.borrow_mut() = apps;
        refresh_command.set_text("Select an application");
        refresh_copy.set_sensitive(false);
        refresh_file.set_visible(false);
    });
    window.set_child(Some(&content));
    window.present();
    search.grab_focus();
}
