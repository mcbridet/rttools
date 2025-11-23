use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::analyzer::{TapeAnalysis, TapeFile};
use crate::output::OutputOptions;
use rtsimh::{AUTHOR, VERSION};

const TICK_RATE: Duration = Duration::from_millis(250);

pub struct App {
    analysis: TapeAnalysis,
    preview_opts: OutputOptions,
    selected_file: usize,
    selected_record: usize,
    should_quit: bool,
    last_tick: Instant,
}

impl App {
    pub fn new(analysis: TapeAnalysis, preview_opts: OutputOptions) -> Self {
        Self {
            analysis,
            preview_opts,
            selected_file: 0,
            selected_record: 0,
            should_quit: false,
            last_tick: Instant::now(),
        }
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Resize(_, _) => {}
            Event::Mouse(_) => {}
            Event::FocusGained | Event::FocusLost | Event::Paste(_) => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == crossterm::event::KeyEventKind::Release {
            return;
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => self.should_quit = true,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => self.should_quit = true,
            (KeyCode::Down, _) => self.next_record(),
            (KeyCode::Up, _) => self.previous_record(),
            (KeyCode::Right, _) => self.next_file(),
            (KeyCode::Left, _) => self.previous_file(),
            _ => {}
        }
    }

    fn next_file(&mut self) {
        if self.analysis.files.is_empty() {
            return;
        }
        self.selected_file = (self.selected_file + 1) % self.analysis.files.len();
        self.selected_record = 0;
    }

    fn previous_file(&mut self) {
        if self.analysis.files.is_empty() {
            return;
        }
        if self.selected_file == 0 {
            self.selected_file = self.analysis.files.len() - 1;
        } else {
            self.selected_file -= 1;
        }
        self.selected_record = 0;
    }

    fn next_record(&mut self) {
        if let Some(file) = self.current_file() {
            if file.records.is_empty() {
                return;
            }
            self.selected_record = (self.selected_record + 1) % file.records.len();
        }
    }

    fn previous_record(&mut self) {
        if let Some(file) = self.current_file() {
            if file.records.is_empty() {
                return;
            }
            if self.selected_record == 0 {
                self.selected_record = file.records.len() - 1;
            } else {
                self.selected_record -= 1;
            }
        }
    }

    fn current_file(&self) -> Option<&TapeFile> {
        self.analysis.files.get(self.selected_file)
    }

    pub fn on_tick(&mut self) {
        self.last_tick = Instant::now();
    }

    pub fn draw(&self, frame: &mut ratatui::Frame<'_>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(5)])
            .split(frame.size());

        frame.render_widget(self.summary_widget(), chunks[0]);
        let mut list_state = ListState::default();
        if !self.analysis.files.is_empty() {
            let idx = self.selected_file.min(self.analysis.files.len() - 1);
            list_state.select(Some(idx));
        }
        frame.render_stateful_widget(self.files_widget(), chunks[1], &mut list_state);
    }

    fn summary_widget(&self) -> Paragraph<'_> {
        let mut lines = vec![
            Line::from(format!("ACMS rtinfo v{} — {}", VERSION, AUTHOR)),
            Line::from(vec![Span::raw(format!(
                "Files: {}  Records: {}  Data bytes: {}",
                format_number(self.analysis.totals.files),
                format_number(self.analysis.totals.records),
                format_number(self.analysis.totals.data_bytes)
            ))]),
        ];

        if let Some(summary) = &self.analysis.tape_summary {
            if !summary.platforms.is_empty() {
                lines.push(Line::from(format!(
                    "Platforms: {}",
                    summary
                        .platforms
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
            if !summary.formats.is_empty() {
                lines.push(Line::from(format!(
                    "Formats: {}",
                    summary
                        .formats
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
            if !summary.details.is_empty() {
                lines.push(Line::from("Details:"));
                const MAX_DETAILS: usize = 4;
                for detail in summary.details.iter().take(MAX_DETAILS) {
                    lines.push(Line::from(format!("  - {detail}")));
                }
                if summary.details.len() > MAX_DETAILS {
                    lines.push(Line::from(format!(
                        "  (+{} more)",
                        summary.details.len() - MAX_DETAILS
                    )));
                }
            }
        }

        lines.push(Line::from(format!(
            "Preview defaults → binary: {}  ascii: {}  labels: {}",
            bool_flag(self.preview_opts.show_binary),
            bool_flag(self.preview_opts.show_ascii),
            bool_flag(self.preview_opts.show_labels)
        )));

        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Tape Summary"))
    }

    fn files_widget(&self) -> List<'_> {
        let items = if self.analysis.files.is_empty() {
            vec![ListItem::new("(no files parsed)")]
        } else {
            self.analysis
                .files
                .iter()
                .enumerate()
                .map(|(idx, file)| {
                    ListItem::new(format!(
                        "File {idx:02}: {} records / {} bytes",
                        file.records.len(),
                        file.data_bytes
                    ))
                })
                .collect::<Vec<_>>()
        };

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Files"))
            .highlight_style(Style::default().add_modifier(Modifier::BOLD))
            .highlight_symbol("→ ");

        list
    }
}

pub fn run_app(analysis: TapeAnalysis, preview_opts: OutputOptions) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let res = run_loop(&mut terminal, analysis, preview_opts);
    shutdown_terminal(terminal)?;
    res
}

fn run_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    analysis: TapeAnalysis,
    preview_opts: OutputOptions,
) -> Result<()> {
    let mut app = App::new(analysis, preview_opts);
    loop {
        terminal.draw(|f| app.draw(f))?;

        let timeout = TICK_RATE
            .checked_sub(app.last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            let event = event::read()?;
            app.handle_event(event);
        }

        if app.last_tick.elapsed() >= TICK_RATE {
            app.on_tick();
        }

        if app.should_quit() {
            break;
        }
    }
    Ok(())
}

fn shutdown_terminal(mut terminal: Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn bool_flag(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn format_number<T: ToString>(value: T) -> String {
    let mut text = value.to_string();
    let mut idx = text.len() as isize - 3;
    while idx > 0 {
        text.insert(idx as usize, ',');
        idx -= 3;
    }
    text
}
