use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::protocol::ResultItem;

const PAGE_STEP: usize = 10;
const MIN_WIDTH: u16 = 44;
const MIN_HEIGHT: u16 = 12;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ViewState {
    Starting,
    Ready,
    Searching,
    Results,
    Empty,
    Error(String),
    Fatal(String),
    ShuttingDown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    None,
    Submit(String),
    ToggleFeedback,
    Restart,
    Open { id: String, target: String },
    Copy { id: String, value: String },
    Select(String),
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    Query,
    Results,
}

#[derive(Debug)]
pub struct App {
    query: String,
    submitted_query: Option<String>,
    state: ViewState,
    results: Vec<ResultItem>,
    selected: usize,
    focus: Focus,
    help: bool,
    chosen_id: Option<String>,
    degraded_routes: Vec<String>,
    notice: Option<String>,
    feedback_enabled: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            submitted_query: None,
            state: ViewState::Starting,
            results: Vec::new(),
            selected: 0,
            focus: Focus::Query,
            help: false,
            chosen_id: None,
            degraded_routes: Vec::new(),
            notice: None,
            feedback_enabled: false,
        }
    }

    #[cfg(test)]
    pub fn state(&self) -> &ViewState {
        &self.state
    }

    #[cfg(test)]
    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn results(&self) -> &[ResultItem] {
        &self.results
    }

    #[cfg(test)]
    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn selected_result(&self) -> Option<&ResultItem> {
        self.results.get(self.selected)
    }

    #[cfg(test)]
    pub fn chosen_id(&self) -> Option<&str> {
        self.chosen_id.as_deref()
    }

    pub fn ready(&mut self) {
        self.state = ViewState::Ready;
        self.notice = None;
    }

    pub(crate) fn begin_search(&mut self, query: String) {
        self.submitted_query = Some(query);
        self.results.clear();
        self.selected = 0;
        self.degraded_routes.clear();
        self.notice = None;
        self.state = ViewState::Searching;
    }

    pub fn show_results(&mut self, results: Vec<ResultItem>, degraded_routes: Vec<String>) {
        self.results = results;
        self.selected = 0;
        self.degraded_routes = degraded_routes;
        self.notice = None;
        self.state = if self.results.is_empty() {
            self.focus = Focus::Query;
            ViewState::Empty
        } else {
            self.focus = Focus::Results;
            ViewState::Results
        };
    }

    pub fn show_error(&mut self, message: impl Into<String>, fatal: bool) {
        let message = message.into();
        self.results.clear();
        self.selected = 0;
        self.focus = Focus::Query;
        self.notice = None;
        self.state = if fatal {
            ViewState::Fatal(message)
        } else {
            ViewState::Error(message)
        };
    }

    pub fn show_notice(&mut self, message: impl Into<String>) {
        self.notice = Some(message.into());
    }

    pub fn set_feedback_enabled(&mut self, enabled: bool) {
        self.feedback_enabled = enabled;
    }

    pub fn shutting_down(&mut self) {
        self.state = ViewState::ShuttingDown;
    }

    #[must_use]
    pub fn handle_event(&mut self, event: Event) -> Action {
        match event {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                self.handle_key(key)
            }
            _ => Action::None,
        }
    }

    #[must_use]
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }

        match key.code {
            KeyCode::Esc if self.help => {
                self.help = false;
                return Action::None;
            }
            KeyCode::Esc if self.focus == Focus::Results => {
                self.focus = Focus::Query;
                return Action::None;
            }
            KeyCode::Esc => return Action::Quit,
            KeyCode::F(1) | KeyCode::Char('?') => {
                self.help = !self.help;
                return Action::None;
            }
            KeyCode::F(2)
                if !matches!(self.state, ViewState::Fatal(_) | ViewState::ShuttingDown) =>
            {
                return Action::ToggleFeedback;
            }
            _ if self.help => return Action::None,
            _ => {}
        }

        if matches!(self.state, ViewState::Fatal(_) | ViewState::ShuttingDown) {
            return match key.code {
                KeyCode::Char('r') if matches!(self.state, ViewState::Fatal(_)) => Action::Restart,
                KeyCode::Char('q') => Action::Quit,
                _ => Action::None,
            };
        }

        match self.focus {
            Focus::Query => self.handle_query_key(key),
            Focus::Results => self.handle_results_key(key),
        }
    }

    fn handle_query_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Enter if !matches!(self.state, ViewState::Starting | ViewState::Searching) => {
                let query = self.query.trim().to_owned();
                if query.is_empty() {
                    self.state = ViewState::Error("Enter a search query.".to_owned());
                    return Action::None;
                }
                self.begin_search(query.clone());
                Action::Submit(query)
            }
            KeyCode::Backspace => {
                self.query.pop();
                Action::None
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.query.clear();
                Action::None
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.query.push(character);
                Action::None
            }
            KeyCode::Tab if !self.results.is_empty() => {
                self.focus = Focus::Results;
                Action::None
            }
            _ => Action::None,
        }
    }

    fn handle_results_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('/') => {
                self.focus = Focus::Query;
                Action::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                Action::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                Action::None
            }
            KeyCode::PageDown => {
                self.move_selection(PAGE_STEP as isize);
                Action::None
            }
            KeyCode::PageUp => {
                self.move_selection(-(PAGE_STEP as isize));
                Action::None
            }
            KeyCode::Home => {
                self.selected = 0;
                Action::None
            }
            KeyCode::End => {
                self.selected = self.results.len().saturating_sub(1);
                Action::None
            }
            KeyCode::Char('o') => {
                self.selected_result()
                    .map_or(Action::None, |item| Action::Open {
                        id: item.id.clone(),
                        target: item
                            .asset_uri
                            .clone()
                            .unwrap_or_else(|| item.source_url.clone()),
                    })
            }
            KeyCode::Char('c') => {
                self.selected_result()
                    .map_or(Action::None, |item| Action::Copy {
                        id: item.id.clone(),
                        value: item
                            .text
                            .clone()
                            .or_else(|| item.caption.clone())
                            .or_else(|| item.asset_uri.clone())
                            .unwrap_or_else(|| item.source_url.clone()),
                    })
            }
            KeyCode::Char('s') | KeyCode::Enter => {
                let Some(id) = self.selected_result().map(|item| item.id.clone()) else {
                    return Action::None;
                };
                self.chosen_id = Some(id.clone());
                Action::Select(id)
            }
            KeyCode::Char('q') => Action::Quit,
            _ => Action::None,
        }
    }

    fn move_selection(&mut self, offset: isize) {
        let last = self.results.len().saturating_sub(1);
        self.selected = self.selected.saturating_add_signed(offset).min(last);
    }
}

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        frame.render_widget(
            Paragraph::new("Terminal too small\nResize to at least 44×12\nEsc or Ctrl-C quits")
                .alignment(Alignment::Center)
                .block(Block::bordered().title(" MamboMeme ")),
            area,
        );
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(area);

    let query_style = if app.focus == Focus::Query {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    frame.render_widget(
        Paragraph::new(app.query.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(query_style)
                .title(" Search "),
        ),
        rows[0],
    );

    match &app.state {
        ViewState::Starting => render_message(frame, rows[1], "Starting", "Loading corpus…"),
        ViewState::Ready => render_message(
            frame,
            rows[1],
            "Ready",
            "Type a query and press Enter to search.",
        ),
        ViewState::Searching => render_message(
            frame,
            rows[1],
            "Searching",
            &format!(
                "Searching for “{}”…",
                app.submitted_query.as_deref().unwrap_or_default()
            ),
        ),
        ViewState::Results => render_results(frame, rows[1], app),
        ViewState::Empty => render_message(
            frame,
            rows[1],
            "No results",
            "No eligible memes matched. Try a person, template, quote, or topic.",
        ),
        ViewState::Error(message) => render_message(frame, rows[1], "Search error", message),
        ViewState::Fatal(message) => render_message(
            frame,
            rows[1],
            "Fatal error",
            &format!("{message}\nPress r to restart the worker or q to quit."),
        ),
        ViewState::ShuttingDown => render_message(
            frame,
            rows[1],
            "Shutting down",
            "Closing the search worker…",
        ),
    }

    let mut footer = match app.focus {
        Focus::Query => "Enter search  Tab results  F1 help  Esc quit".to_owned(),
        Focus::Results => "↑↓/Pg navigate  o open  c copy  s select  / query  q quit".to_owned(),
    };
    if !app.degraded_routes.is_empty() {
        footer.push_str("  degraded: ");
        footer.push_str(&app.degraded_routes.join(", "));
    }
    if let Some(notice) = &app.notice {
        footer.push_str("  · ");
        footer.push_str(notice);
    }
    footer.push_str(if app.feedback_enabled {
        "  feedback: on"
    } else {
        "  feedback: off"
    });
    frame.render_widget(Paragraph::new(footer).wrap(Wrap { trim: true }), rows[2]);

    if app.help {
        render_help(frame, centered(area, 70, 60));
    }
}

fn render_message(frame: &mut Frame<'_>, area: Rect, title: &str, message: &str) {
    frame.render_widget(
        Paragraph::new(message)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(Block::bordered().title(format!(" {title} "))),
        area,
    );
}

fn render_results(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let panes = if area.width >= 96 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(area)
    };

    let items = app
        .results
        .iter()
        .map(|item| {
            let chosen = if app.chosen_id.as_deref() == Some(item.id.as_str()) {
                " [selected]"
            } else {
                ""
            };
            ListItem::new(format!("{}. {}{chosen}", item.rank, item.title))
        })
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(Block::bordered().title(format!(" Results ({}) ", app.results.len())))
        .highlight_symbol("> ")
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(list, panes[0], &mut state);

    frame.render_widget(
        Paragraph::new(preview(app.selected_result()))
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(" Preview ")),
        panes[1],
    );
}

fn preview(item: Option<&ResultItem>) -> Text<'static> {
    let Some(item) = item else {
        return Text::from("No result selected.");
    };
    let description = item
        .text
        .as_deref()
        .or(item.caption.as_deref())
        .unwrap_or("No text or caption is available.");
    let asset = item
        .asset_uri
        .as_deref()
        .map(|uri| format!("Asset: {uri}"))
        .unwrap_or_else(|| "No image preview; showing portable metadata.".to_owned());
    let mut lines = vec![
        Line::styled(
            item.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::from(description.to_owned()),
        Line::from(asset),
        Line::from(format!("Kind: {}  Language: {}", item.kind, item.language)),
    ];
    if !item.people.is_empty() {
        lines.push(Line::from(format!("People: {}", item.people.join(", "))));
    }
    if let Some(template) = &item.template {
        lines.push(Line::from(format!("Template: {template}")));
    }
    if !item.tags.is_empty() {
        lines.push(Line::from(format!("Tags: {}", item.tags.join(", "))));
    }
    lines.extend([
        Line::from(format!("Source: {}", item.source)),
        Line::from(format!("Attribution: {}", item.attribution)),
        Line::from(format!("ID: {}", item.id)),
    ]);
    Text::from(lines)
}

fn render_help(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(
            "Query: type, Backspace, Ctrl-U clear, Enter search\n\
             Results: ↑/↓, PgUp/PgDn, Home/End, o open, c copy, s select\n\
             Tab or / changes focus · F2 feedback · Esc backs out · Ctrl-C quits\n\
             Fatal error: r restarts the worker",
        )
        .wrap(Wrap { trim: true })
        .block(Block::bordered().title(" Help ")),
        area,
    );
}

fn centered(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - height_percent) / 2),
        Constraint::Percentage(height_percent),
        Constraint::Percentage((100 - height_percent) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - width_percent) / 2),
        Constraint::Percentage(width_percent),
        Constraint::Percentage((100 - width_percent) / 2),
    ])
    .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Scores;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn result(index: usize) -> ResultItem {
        ResultItem {
            asset_uri: None,
            attribution: "Fixture authors".to_owned(),
            caption: None,
            dataset_version: "fixture-v1".to_owned(),
            id: format!("item-{index}"),
            kind: "text".to_owned(),
            language: "en".to_owned(),
            matched_fields: vec!["people".to_owned()],
            people: vec!["John Cena".to_owned()],
            rank: index + 1,
            retriever_version: "lexical-v1".to_owned(),
            routes: vec!["lexical".to_owned()],
            safe: true,
            scores: Scores {
                dense_rank: None,
                fused: 1.0,
                lexical_rank: Some(index + 1),
            },
            source: "fixture".to_owned(),
            source_url: "https://example.invalid/meme".to_owned(),
            tags: vec!["wrestling".to_owned()],
            template: None,
            text: Some(format!("You can’t see me {index}")),
            title: format!("约翰 Cena result {index}"),
        }
    }

    fn render_text(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn query_editing_is_unicode_safe_and_submission_is_explicit() {
        let mut app = App::new();
        app.ready();
        for character in "约翰 q🎭".chars() {
            assert_eq!(app.handle_key(key(KeyCode::Char(character))), Action::None);
        }
        let _ = app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.query(), "约翰 q");
        assert_eq!(
            app.handle_key(key(KeyCode::Enter)),
            Action::Submit("约翰 q".to_owned())
        );
        assert_eq!(app.state(), &ViewState::Searching);
        assert_eq!(app.handle_key(key(KeyCode::Enter)), Action::None);

        let _ = app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(app.query(), "");
    }

    #[test]
    fn result_navigation_clamps_and_actions_target_the_highlighted_id() {
        let mut app = App::new();
        app.show_results((0..12).map(result).collect(), Vec::new());

        let _ = app.handle_key(key(KeyCode::PageDown));
        assert_eq!(app.selected_result().unwrap().id, "item-10");
        let _ = app.handle_key(key(KeyCode::PageDown));
        assert_eq!(app.selected_result().unwrap().id, "item-11");
        let _ = app.handle_key(key(KeyCode::PageUp));
        assert_eq!(app.selected_result().unwrap().id, "item-1");
        assert_eq!(
            app.handle_key(key(KeyCode::Char('o'))),
            Action::Open {
                id: "item-1".to_owned(),
                target: "https://example.invalid/meme".to_owned(),
            }
        );
        assert_eq!(
            app.handle_key(key(KeyCode::Char('s'))),
            Action::Select("item-1".to_owned())
        );
        assert_eq!(app.chosen_id(), Some("item-1"));
    }

    #[test]
    fn escape_backs_out_before_quitting_and_control_c_always_quits() {
        let mut app = App::new();
        app.show_results(vec![result(0)], Vec::new());
        assert_eq!(app.focus(), Focus::Results);
        assert_eq!(app.handle_key(key(KeyCode::Esc)), Action::None);
        assert_eq!(app.focus(), Focus::Query);
        assert_eq!(app.handle_key(key(KeyCode::Esc)), Action::Quit);
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Action::Quit
        );
    }

    #[test]
    fn worker_callbacks_cover_empty_recoverable_and_fatal_states() {
        let mut app = App::new();
        app.show_results(Vec::new(), vec!["dense".to_owned()]);
        assert_eq!(app.state(), &ViewState::Empty);
        app.show_error("invalid filter", false);
        assert_eq!(app.state(), &ViewState::Error("invalid filter".to_owned()));
        app.show_error("worker exited", true);
        assert_eq!(app.state(), &ViewState::Fatal("worker exited".to_owned()));
        assert_eq!(app.handle_key(key(KeyCode::Char('r'))), Action::Restart);
    }

    #[test]
    fn feedback_toggle_is_explicit_and_visible() {
        let mut app = App::new();
        app.ready();
        assert_eq!(app.handle_key(key(KeyCode::F(2))), Action::ToggleFeedback);
        app.set_feedback_enabled(true);
        assert!(render_text(&app, 80, 18).contains("feedback: on"));

        app.show_error("worker exited", true);
        assert_eq!(app.handle_key(key(KeyCode::F(2))), Action::None);
    }

    #[test]
    fn test_backend_renders_unicode_fallback_long_fields_and_tiny_resize() {
        let mut item = result(0);
        item.title = format!("约翰 Cena {}", "very-long-title ".repeat(20));
        item.text = None;
        let mut app = App::new();
        app.show_results(vec![item], vec!["dense".to_owned()]);

        let wide = render_text(&app, 120, 30);
        assert!(wide.contains('约'));
        assert!(wide.contains("Cena"));
        assert!(wide.contains("No image preview"));
        assert!(wide.contains("degraded: dense"));

        let narrow = render_text(&app, 60, 18);
        assert!(narrow.contains("Preview"));
        let tiny = render_text(&app, 30, 8);
        assert!(tiny.contains("Terminal too small"));
    }
}
