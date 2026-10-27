use std::env;
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table, TableState, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::catalog::Catalog;
use crate::package::Package;

pub(crate) fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pacat: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "pacat - browse installed Linux packages and preview uninstall commands\n\nUsage: pacat [--list] [search words...]\n\n  --list    Print matching packages and commands as a tab-separated table\n  -h, --help  Show this help\n\nType to search. Up/Down or PageUp/PageDown to move.\nEnter prints the selected command and exits. Esc clears search or quits.\nCtrl+C or Ctrl+Q quits. Uninstall commands are never executed.\n\nSupports Pacman, dpkg, RPM, Flatpak and Snap.\nNative package lists include libraries and system packages.\nRedirected output automatically uses --list mode."
        );
        return Ok(());
    }
    if let Some(argument) = args
        .iter()
        .find(|arg| arg.starts_with('-') && *arg != "--list")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown option {argument:?}; use --help"),
        ));
    }
    let list = args.iter().any(|arg| arg == "--list")
        || !io::stdout().is_terminal()
        || !io::stdin().is_terminal();
    let query = args
        .iter()
        .filter(|arg| *arg != "--list")
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let catalog = Catalog::scan()?;
    for warning in &catalog.warnings {
        eprintln!("pacat: {warning}");
    }
    let mut app = App::new(catalog, query);
    if list {
        let mut stdout = io::BufWriter::new(io::stdout().lock());
        writeln!(stdout, "NAME\tVERSION\tSOURCE\tUNINSTALL COMMAND")?;
        for index in &app.filtered {
            let package = &app.catalog.packages[*index];
            writeln!(
                stdout,
                "{}\t{}\t{}\t{}",
                package.name,
                package.version,
                package.source,
                package.uninstall_command()
            )?;
        }
        return stdout.flush();
    }
    let mut terminal = ratatui::try_init()?;
    let result = app.interact(&mut terminal);
    ratatui::restore();
    if let Some(command) = result? {
        println!("{command}");
    }
    Ok(())
}

struct App {
    catalog: Catalog,
    query: String,
    filtered: Vec<usize>,
    selection: TableState,
}

impl App {
    fn new(catalog: Catalog, query: String) -> Self {
        let mut app = Self {
            catalog,
            query,
            filtered: Vec::new(),
            selection: TableState::default(),
        };
        app.filter();
        app
    }

    fn filter(&mut self) {
        self.filtered = self
            .catalog
            .packages
            .iter()
            .enumerate()
            .filter(|(_, package)| package.matches(&self.query))
            .map(|(index, _)| index)
            .collect();
        self.selection = TableState::default();
        self.selection
            .select((!self.filtered.is_empty()).then_some(0));
    }

    fn selected(&self) -> Option<&Package> {
        self.selection
            .selected()
            .and_then(|index| self.filtered.get(index))
            .map(|index| &self.catalog.packages[*index])
    }

    fn move_selection(&mut self, offset: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let index = self
            .selection
            .selected()
            .unwrap_or(0)
            .saturating_add_signed(offset)
            .min(self.filtered.len() - 1);
        self.selection.select(Some(index));
    }

    fn interact(&mut self, terminal: &mut DefaultTerminal) -> io::Result<Option<String>> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                match key.code {
                    KeyCode::Char('c' | 'q') => return Ok(None),
                    KeyCode::Char('u') => {
                        self.query.clear();
                        self.filter();
                    }
                    _ => {}
                }
                continue;
            }
            match key.code {
                KeyCode::Esc if self.query.is_empty() => return Ok(None),
                KeyCode::Esc => {
                    self.query.clear();
                    self.filter();
                }
                KeyCode::Enter => {
                    if let Some(package) = self.selected() {
                        return Ok(Some(package.uninstall_command()));
                    }
                }
                KeyCode::Up => self.move_selection(-1),
                KeyCode::Down => self.move_selection(1),
                KeyCode::PageUp => self.move_selection(-10),
                KeyCode::PageDown => self.move_selection(10),
                KeyCode::Home => self
                    .selection
                    .select((!self.filtered.is_empty()).then_some(0)),
                KeyCode::End => self.selection.select(self.filtered.len().checked_sub(1)),
                KeyCode::Backspace => {
                    self.query.pop();
                    self.filter();
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::ALT) => {
                    self.query.push(character);
                    self.filter();
                }
                _ => {}
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame<'_>) {
        let accent = Color::Cyan;
        let [heading, search, packages, preview, footer] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(5),
            Constraint::Length(2),
        ])
        .areas(frame.area());
        frame.render_widget(
            Paragraph::new(Line::from(format!(
                " pacat   {} / {} installed packages   |   command preview only",
                self.filtered.len(),
                self.catalog.packages.len()
            )))
            .style(Style::default().fg(accent).add_modifier(Modifier::BOLD)),
            heading,
        );
        frame.render_widget(
            Paragraph::new(self.query.as_str())
                .block(Block::bordered().title(" Search by name, version or source ")),
            search,
        );
        let rows = self.filtered.iter().map(|index| {
            let package = &self.catalog.packages[*index];
            Row::new(vec![
                package.name.clone(),
                package.version.clone(),
                package.source.to_string(),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Percentage(48),
                Constraint::Percentage(28),
                Constraint::Percentage(24),
            ],
        )
        .header(
            Row::new(["Application / package", "Version", "Source"])
                .style(Style::default().fg(accent))
                .bottom_margin(1),
        )
        .block(Block::bordered().title(" Installed "))
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
        frame.render_stateful_widget(table, packages, &mut self.selection);
        let command = self
            .selected()
            .map(Package::uninstall_command)
            .unwrap_or_else(|| "No matching packages".into());
        frame.render_widget(
            Paragraph::new(command)
                .style(Style::default().fg(Color::Green))
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(" Uninstall command · displayed only ")),
            preview,
        );
        let warning = if self.catalog.warnings.is_empty() {
            String::new()
        } else {
            format!(
                "{} source query failed; see stderr",
                self.catalog.warnings.len()
            )
        };
        frame.render_widget(Paragraph::new(vec![
            Line::from(" Type to search | Up/Down move | Enter print & exit | Esc clear/quit | Ctrl+Q quit"),
            Line::from(warning),
        ]).block(Block::default().borders(Borders::NONE)), footer);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::App;
    use crate::catalog::Catalog;
    use crate::package::{Package, Source};

    fn app() -> App {
        App::new(
            Catalog {
                packages: vec![
                    Package::new("firefox", "Firefox", "144", Source::Pacman)
                        .expect("valid package"),
                    Package::new(
                        "org.example.App",
                        "Example App",
                        "1",
                        Source::Flatpak {
                            installation: "user".into(),
                        },
                    )
                    .expect("valid package"),
                ],
                warnings: Vec::new(),
            },
            String::new(),
        )
    }

    #[test]
    fn filtering_and_navigation_keep_selection_valid() {
        let mut app = app();
        app.move_selection(100);
        assert_eq!(app.selection.selected(), Some(1));
        app.query = "FIRE pacman".into();
        app.filter();
        assert_eq!(app.selected().expect("matching package").id, "firefox");
        app.query = "missing".into();
        app.filter();
        app.move_selection(-1);
        assert!(
            app.selected().is_none(),
            "empty search must not retain a stale command"
        );
    }

    #[test]
    fn rendering_handles_small_terminals_and_empty_searches() {
        let mut app = app();
        for (width, height) in [(100, 30), (30, 10), (1, 1)] {
            let mut terminal =
                Terminal::new(TestBackend::new(width, height)).expect("test terminal");
            terminal
                .draw(|frame| app.draw(frame))
                .expect("render packages");
            app.query = "missing".into();
            app.filter();
            terminal
                .draw(|frame| app.draw(frame))
                .expect("render empty search");
        }
    }
}
