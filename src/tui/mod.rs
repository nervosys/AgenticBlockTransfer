// TUI mode — Interactive terminal UI using ratatui

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::*,
};
use std::io;
use std::path::PathBuf;

use crate::core::device::{self, DeviceInfo};
use crate::core::progress::{OperationPhase, Progress, ProgressSnapshot};

/// Valid image file extensions for the TUI file browser filter.
const IMAGE_EXTENSIONS: &[&str] = &[
    "iso", "img", "raw", "bin", "dd", "dsk", "dmg", "vhd", "vhdx", "vmdk",
    "qcow2", "wim", "ffu", "gz", "bz2", "xz", "zst", "zip",
];

/// A single entry in the file browser.
#[derive(Debug, Clone)]
struct BrowserEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    size: u64,
}

/// Application state for the TUI.
struct App {
    state: AppState,
    devices: Vec<DeviceInfo>,
    selected_device: usize,
    source_path: String,
    source_input: String,
    status_message: String,
    progress: Option<Progress>,
    should_quit: bool,
    input_mode: InputMode,
    // File browser state
    browser_path: PathBuf,
    browser_entries: Vec<BrowserEntry>,
    browser_selected: usize,
    browser_show_hidden: bool,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum AppState {
    SelectSource,
    BrowseFile,
    SelectDevice,
    Confirm,
    Writing,
    Complete,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
enum InputMode {
    Normal,
    Editing,
}

impl App {
    fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            state: AppState::SelectSource,
            devices: Vec::new(),
            selected_device: 0,
            source_path: String::new(),
            source_input: String::new(),
            status_message: "Enter the path to the source image file".to_string(),
            progress: None,
            should_quit: false,
            input_mode: InputMode::Editing,
            browser_path: home,
            browser_entries: Vec::new(),
            browser_selected: 0,
            browser_show_hidden: false,
        }
    }

    /// Scan the current `browser_path` and populate `browser_entries`.
    fn refresh_browser(&mut self) {
        self.browser_entries.clear();
        self.browser_selected = 0;

        let entries = match std::fs::read_dir(&self.browser_path) {
            Ok(rd) => rd,
            Err(e) => {
                self.status_message = format!("Cannot read directory: {}", e);
                return;
            }
        };

        let mut dirs: Vec<BrowserEntry> = Vec::new();
        let mut files: Vec<BrowserEntry> = Vec::new();

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files unless toggled
            if !self.browser_show_hidden && name.starts_with('.') {
                continue;
            }

            let path = entry.path();
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map_or(false, |m| m.is_dir());
            let size = meta.as_ref().map_or(0, |m| m.len());

            if is_dir {
                dirs.push(BrowserEntry { name, path, is_dir, size });
            } else {
                // Filter files: only show disk image extensions
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
                    files.push(BrowserEntry { name, path, is_dir, size });
                }
            }
        }

        // Sort alphabetically (case-insensitive)
        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        // Directories first, then image files
        self.browser_entries.extend(dirs);
        self.browser_entries.extend(files);
    }
}

pub async fn run() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    // Load devices
    let enumerator = device::create_enumerator();
    app.devices = enumerator.list_devices().await.unwrap_or_default();

    // Main loop
    let result = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') if app.input_mode == InputMode::Normal => {
                        app.should_quit = true;
                    }
                    KeyCode::Esc => {
                        if app.input_mode == InputMode::Editing {
                            app.input_mode = InputMode::Normal;
                        } else {
                            app.should_quit = true;
                        }
                    }
                    _ => match &app.state {
                        AppState::SelectSource => handle_source_input(app, key.code),
                        AppState::BrowseFile => handle_file_browser(app, key.code),
                        AppState::SelectDevice => handle_device_select(app, key.code),
                        AppState::Confirm => handle_confirm(app, key.code),
                        AppState::Writing => {} // No input during write
                        AppState::Complete | AppState::Error(_) => {
                            if key.code == KeyCode::Enter || key.code == KeyCode::Char('q') {
                                app.should_quit = true;
                            }
                        }
                    },
                }
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_source_input(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Enter => {
            if !app.source_input.is_empty() {
                let path = std::path::PathBuf::from(&app.source_input);
                if path.exists() {
                    app.source_path = app.source_input.clone();
                    app.state = AppState::SelectDevice;
                    app.input_mode = InputMode::Normal;
                    app.status_message =
                        "Select target device (↑/↓ to navigate, Enter to select)".to_string();
                } else {
                    app.status_message = format!("File not found: {}", app.source_input);
                }
            }
        }
        KeyCode::Tab => {
            // Open the file browser
            app.state = AppState::BrowseFile;
            app.input_mode = InputMode::Normal;
            app.refresh_browser();
            app.status_message =
                "↑/↓: navigate | Enter: open/select | Backspace: parent | h: toggle hidden | Esc: back".to_string();
        }
        KeyCode::Char(c) => {
            app.source_input.push(c);
        }
        KeyCode::Backspace => {
            app.source_input.pop();
        }
        _ => {}
    }
}

fn handle_file_browser(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Up => {
            if app.browser_selected > 0 {
                app.browser_selected -= 1;
            }
        }
        KeyCode::Down => {
            if app.browser_selected < app.browser_entries.len().saturating_sub(1) {
                app.browser_selected += 1;
            }
        }
        KeyCode::Enter => {
            if let Some(entry) = app.browser_entries.get(app.browser_selected).cloned() {
                if entry.is_dir {
                    // Navigate into directory
                    app.browser_path = entry.path;
                    app.refresh_browser();
                } else {
                    // Select this file
                    app.source_path = entry.path.to_string_lossy().to_string();
                    app.source_input = app.source_path.clone();
                    app.state = AppState::SelectDevice;
                    app.input_mode = InputMode::Normal;
                    app.status_message =
                        "Select target device (↑/↓ to navigate, Enter to select)".to_string();
                }
            }
        }
        KeyCode::Backspace => {
            // Go to parent directory
            if let Some(parent) = app.browser_path.parent() {
                app.browser_path = parent.to_path_buf();
                app.refresh_browser();
            }
        }
        KeyCode::Char('h') => {
            app.browser_show_hidden = !app.browser_show_hidden;
            app.refresh_browser();
        }
        KeyCode::Esc => {
            // Return to text input
            app.state = AppState::SelectSource;
            app.input_mode = InputMode::Editing;
            app.status_message = "Enter the path to the source image file".to_string();
        }
        _ => {}
    }
}

fn handle_device_select(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Up => {
            if app.selected_device > 0 {
                app.selected_device -= 1;
            }
        }
        KeyCode::Down => {
            if app.selected_device < app.devices.len().saturating_sub(1) {
                app.selected_device += 1;
            }
        }
        KeyCode::Enter => {
            if !app.devices.is_empty() {
                let dev = &app.devices[app.selected_device];
                if dev.is_system {
                    app.status_message = "Cannot write to system drive!".to_string();
                } else {
                    app.state = AppState::Confirm;
                    app.status_message = "Press 'y' to confirm, 'n' to go back".to_string();
                }
            }
        }
        _ => {}
    }
}

fn handle_confirm(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.state = AppState::Writing;
            app.status_message = "Writing...".to_string();
            // In a real implementation, spawn the write task here
            app.progress = Some(Progress::new(100));
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            app.state = AppState::SelectDevice;
            app.status_message = "Select target device".to_string();
        }
        _ => {}
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Min(10),   // Main content
            Constraint::Length(3), // Status bar
            Constraint::Length(3), // Help
        ])
        .split(f.area());

    // Title
    let title = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(
            " abt v{} ",
            env!("CARGO_PKG_VERSION")
        ))
        .title_alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    // Main content
    match &app.state {
        AppState::SelectSource => render_source_input(f, app, chunks[1]),
        AppState::BrowseFile => render_file_browser(f, app, chunks[1]),
        AppState::SelectDevice => render_device_list(f, app, chunks[1]),
        AppState::Confirm => render_confirmation(f, app, chunks[1]),
        AppState::Writing => render_progress(f, app, chunks[1]),
        AppState::Complete => render_complete(f, app, chunks[1]),
        AppState::Error(msg) => render_error(f, msg, chunks[1]),
    }

    // Status bar
    let status = Paragraph::new(app.status_message.clone())
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::ALL).title(" Status "));
    f.render_widget(status, chunks[2]);

    // Help
    let help_text = match app.state {
        AppState::SelectSource => "Type path to image | Tab: browse files | Enter: confirm | Esc: quit",
        AppState::BrowseFile => "↑/↓: navigate | Enter: open/select | Backspace: parent dir | h: hidden | Esc: back",
        AppState::SelectDevice => "↑/↓: navigate | Enter: select | Esc: back",
        AppState::Confirm => "y: confirm write | n: go back | Esc: quit",
        AppState::Writing => "Writing in progress...",
        AppState::Complete | AppState::Error(_) => "Enter/q: quit",
    };
    let help = Paragraph::new(help_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(help, chunks[3]);
}

fn render_source_input(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Source Image ")
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let text_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(inner);

    let label = Paragraph::new("Enter the path to the disk image file:")
        .style(Style::default().fg(Color::White));
    f.render_widget(label, text_chunks[0]);

    let input = Paragraph::new(format!("▎ {}", app.source_input))
        .style(Style::default().fg(Color::Cyan))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
    f.render_widget(input, text_chunks[1]);

    let formats = Paragraph::new(
        "Supported: ISO, IMG, RAW, VHD, VHDX, VMDK, QCOW2, DMG, WIM, FFU\n\
         Compressed: .gz, .bz2, .xz, .zst, .zip (auto-detected)\n\
         Press Tab to open file browser",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(formats, text_chunks[2]);
}

fn render_file_browser(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " Browse: {} ",
            app.browser_path.to_string_lossy()
        ))
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.browser_entries.is_empty() {
        let empty = Paragraph::new("  (empty directory — no image files or folders found)")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, inner);
        return;
    }

    let header = Row::new(vec!["", "Name", "Size"])
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1);

    let rows: Vec<Row> = app
        .browser_entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let icon = if entry.is_dir { "📁" } else { "💿" };
            let size_str = if entry.is_dir {
                "<DIR>".to_string()
            } else {
                humansize::format_size(entry.size, humansize::BINARY)
            };

            let style = if i == app.browser_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if entry.is_dir {
                Style::default().fg(Color::Blue)
            } else {
                Style::default()
            };

            Row::new(vec![icon.to_string(), entry.name.clone(), size_str]).style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Min(30),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = TableState::default();
    state.select(Some(app.browser_selected));
    f.render_stateful_widget(table, inner, &mut state);
}

fn render_device_list(f: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec!["Device", "Name", "Size", "Type", "Removable"])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let rows: Vec<Row> = app
        .devices
        .iter()
        .enumerate()
        .map(|(i, dev)| {
            let size = humansize::format_size(dev.size, humansize::BINARY);
            let style = if dev.is_system {
                Style::default().fg(Color::Red)
            } else if i == app.selected_device {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let marker = if dev.is_system { " [SYS]" } else { "" };

            Row::new(vec![
                format!("{}{}", dev.path, marker),
                dev.name.clone(),
                size,
                format!("{}", dev.device_type),
                if dev.removable { "Yes" } else { "No" }.to_string(),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(25),
            Constraint::Percentage(30),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Target Device ")
            .border_style(Style::default().fg(Color::Green)),
    )
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = TableState::default();
    state.select(Some(app.selected_device));
    f.render_stateful_widget(table, area, &mut state);
}

fn render_confirmation(f: &mut Frame, app: &App, area: Rect) {
    let dev = &app.devices[app.selected_device];
    let size = humansize::format_size(dev.size, humansize::BINARY);

    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Source: ", Style::default().fg(Color::Yellow)),
            Span::raw(&app.source_path),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Target: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{} ({}) [{}]", dev.path, dev.name, size)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  ⚠  ALL DATA ON THE TARGET DEVICE WILL BE DESTROYED",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  Press 'y' to confirm, 'n' to go back"),
    ];

    let paragraph = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Confirm Write ")
            .border_style(Style::default().fg(Color::Red)),
    );
    f.render_widget(paragraph, area);
}

fn render_progress(f: &mut Frame, app: &App, area: Rect) {
    let snap = app
        .progress
        .as_ref()
        .map(|p| p.snapshot())
        .unwrap_or(ProgressSnapshot {
            phase: OperationPhase::Preparing,
            bytes_written: 0,
            bytes_total: 100,
            percent: 0.0,
            elapsed_secs: 0.0,
            speed_bytes_per_sec: 0.0,
            eta_secs: None,
        });

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Writing ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(inner);

    let phase = Paragraph::new(format!("  Phase: {}", snap.phase))
        .style(Style::default().fg(Color::White));
    f.render_widget(phase, chunks[0]);

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
        .percent(snap.percent.min(100.0) as u16)
        .label(format!("{:.1}%", snap.percent));
    f.render_widget(gauge, chunks[1]);

    let written = humansize::format_size(snap.bytes_written, humansize::BINARY);
    let total = humansize::format_size(snap.bytes_total, humansize::BINARY);
    let speed = humansize::format_size(snap.speed_bytes_per_sec as u64, humansize::BINARY);
    let eta = snap
        .eta_secs
        .map(|e| format!("{:.0}s", e))
        .unwrap_or_else(|| "calculating...".to_string());

    let stats = Paragraph::new(format!(
        "  {} / {} | {}/s | ETA: {}",
        written, total, speed, eta
    ))
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(stats, chunks[2]);
}

fn render_complete(f: &mut Frame, _app: &App, area: Rect) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  ✓ Write completed successfully!",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  The device is safe to remove."),
        Line::from(""),
        Line::from("  Press Enter or 'q' to exit."),
    ];

    let paragraph = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Complete ")
            .border_style(Style::default().fg(Color::Green)),
    );
    f.render_widget(paragraph, area);
}

fn render_error(f: &mut Frame, msg: &str, area: Rect) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  ✗ Write failed!",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!("  Error: {}", msg)),
        Line::from(""),
        Line::from("  Press Enter or 'q' to exit."),
    ];

    let paragraph = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Error ")
            .border_style(Style::default().fg(Color::Red)),
    );
    f.render_widget(paragraph, area);
}
