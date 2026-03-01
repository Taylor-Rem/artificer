use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste,
        EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent,
        MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame, Terminal,
};

use artificer_shared::events::ChatEvent;
use crate::client::ApiClient;

// ============================================================================
// THEME
// ============================================================================

struct Theme;

impl Theme {
    // Background / chrome
    const BG: Color = Color::Rgb(13, 13, 18);
    const BORDER: Color = Color::Rgb(45, 45, 60);
    const BORDER_ACTIVE: Color = Color::Rgb(90, 90, 130);

    // Text
    const TEXT: Color = Color::Rgb(210, 210, 220);
    const TEXT_DIM: Color = Color::Rgb(110, 110, 130);
    const TEXT_BRIGHT: Color = Color::Rgb(240, 240, 250);

    // Roles
    const USER: Color = Color::Rgb(130, 180, 255);       // soft blue
    const ASSISTANT: Color = Color::Rgb(180, 240, 180);  // soft green
    const TOOL_CALL: Color = Color::Rgb(255, 200, 100);  // amber
    const TOOL_RESULT: Color = Color::Rgb(160, 160, 180); // muted purple-grey
    const TASK_SWITCH: Color = Color::Rgb(200, 140, 255); // violet
    const ERROR: Color = Color::Rgb(255, 100, 100);       // red

    // Input
    const INPUT_BG: Color = Color::Rgb(20, 20, 30);
    const CURSOR: Color = Color::Rgb(130, 180, 255);

    // Status
    const STATUS_BG: Color = Color::Rgb(25, 25, 40);
    const STATUS_OK: Color = Color::Rgb(100, 220, 120);
    const STATUS_BUSY: Color = Color::Rgb(255, 180, 60);
}

// ============================================================================
// MESSAGE MODEL
// ============================================================================

#[derive(Debug, Clone)]
enum MessageKind {
    User,
    Assistant,
    ToolCall { tool: String, args_preview: String },
    ToolResult { tool: String, truncated: bool },
    TaskSwitch { from: String, to: String },
    Error,
    System,
}

#[derive(Debug, Clone)]
struct ChatMessage {
    kind: MessageKind,
    content: String,
    /// For in-progress assistant streaming — still receiving chunks
    streaming: bool,
}

impl ChatMessage {
    fn user(content: String) -> Self {
        Self { kind: MessageKind::User, content, streaming: false }
    }

    fn assistant_streaming() -> Self {
        Self {
            kind: MessageKind::Assistant,
            content: String::new(),
            streaming: true,
        }
    }

    fn tool_call(tool: String, args_preview: String, content: String) -> Self {
        Self {
            kind: MessageKind::ToolCall { tool, args_preview },
            content,
            streaming: false,
        }
    }

    fn tool_result(tool: String, truncated: bool, content: String) -> Self {
        Self {
            kind: MessageKind::ToolResult { tool, truncated },
            content,
            streaming: false,
        }
    }

    fn task_switch(from: String, to: String) -> Self {
        Self {
            kind: MessageKind::TaskSwitch { from: from.clone(), to: to.clone() },
            content: format!("{} → {}", from, to),
            streaming: false,
        }
    }

    fn error(content: String) -> Self {
        Self { kind: MessageKind::Error, content, streaming: false }
    }

    fn system(content: String) -> Self {
        Self { kind: MessageKind::System, content, streaming: false }
    }
}

// ============================================================================
// INPUT STATE
// ============================================================================

struct InputState {
    /// Lines of the current input buffer
    lines: Vec<String>,
    /// Cursor position: (line_index, char_index)
    cursor: (usize, usize),
    /// History of sent messages (newest last)
    history: VecDeque<String>,
    /// Current position in history browsing (-1 = not browsing)
    history_pos: Option<usize>,
    /// Saved current buffer when browsing history
    history_draft: Option<Vec<String>>,
}

impl InputState {
    fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor: (0, 0),
            history: VecDeque::new(),
            history_pos: None,
            history_draft: None,
        }
    }

    fn current_text(&self) -> String {
        self.lines.join("\n")
    }

    fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.is_empty())
    }

    fn insert_char(&mut self, c: char) {
        let (row, col) = self.cursor;
        self.lines[row].insert(col, c);
        self.cursor = (row, col + c.len_utf8());
    }

    fn insert_newline(&mut self) {
        let (row, col) = self.cursor;
        let rest = self.lines[row].split_off(col);
        self.lines.insert(row + 1, rest);
        self.cursor = (row + 1, 0);
    }

    fn insert_str(&mut self, s: &str) {
        for (i, part) in s.split('\n').enumerate() {
            if i > 0 {
                self.insert_newline();
            }
            for c in part.chars() {
                self.insert_char(c);
            }
        }
    }

    fn backspace(&mut self) {
        let (row, col) = self.cursor;
        if col > 0 {
            // Find previous char boundary
            let line = &self.lines[row];
            let prev_col = line[..col]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.lines[row].remove(prev_col);
            self.cursor = (row, prev_col);
        } else if row > 0 {
            // Merge with previous line
            let current = self.lines.remove(row);
            let prev_len = self.lines[row - 1].len();
            self.lines[row - 1].push_str(&current);
            self.cursor = (row - 1, prev_len);
        }
    }

    fn delete(&mut self) {
        let (row, col) = self.cursor;
        let line_len = self.lines[row].len();
        if col < line_len {
            // Find next char boundary
            let c = self.lines[row][col..].chars().next().unwrap();
            self.lines[row].remove(col);
            let _ = c;
        } else if row + 1 < self.lines.len() {
            // Merge next line into current
            let next = self.lines.remove(row + 1);
            self.lines[row].push_str(&next);
        }
    }

    fn move_left(&mut self) {
        let (row, col) = self.cursor;
        if col > 0 {
            let prev_col = self.lines[row][..col]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.cursor = (row, prev_col);
        } else if row > 0 {
            self.cursor = (row - 1, self.lines[row - 1].len());
        }
    }

    fn move_right(&mut self) {
        let (row, col) = self.cursor;
        let line_len = self.lines[row].len();
        if col < line_len {
            let next_col = col + self.lines[row][col..].chars().next().map_or(0, |c| c.len_utf8());
            self.cursor = (row, next_col);
        } else if row + 1 < self.lines.len() {
            self.cursor = (row + 1, 0);
        }
    }

    fn move_up(&mut self) {
        let (row, col) = self.cursor;
        if row > 0 {
            let new_col = col.min(self.lines[row - 1].len());
            self.cursor = (row - 1, new_col);
        }
    }

    fn move_down(&mut self) {
        let (row, col) = self.cursor;
        if row + 1 < self.lines.len() {
            let new_col = col.min(self.lines[row + 1].len());
            self.cursor = (row + 1, new_col);
        }
    }

    fn move_home(&mut self) {
        self.cursor.1 = 0;
    }

    fn move_end(&mut self) {
        let row = self.cursor.0;
        self.cursor.1 = self.lines[row].len();
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next_pos = match self.history_pos {
            None => {
                // Save current draft
                self.history_draft = Some(self.lines.clone());
                0
            }
            Some(p) if p + 1 < self.history.len() => p + 1,
            Some(p) => p,
        };
        self.history_pos = Some(next_pos);
        // history is newest-last, so index from the end
        let idx = self.history.len() - 1 - next_pos;
        let text = self.history[idx].clone();
        self.lines = text.split('\n').map(String::from).collect();
        if self.lines.is_empty() {
            self.lines = vec![String::new()];
        }
        let last = self.lines.len() - 1;
        self.cursor = (last, self.lines[last].len());
    }

    fn history_down(&mut self) {
        match self.history_pos {
            None => {}
            Some(0) => {
                self.history_pos = None;
                self.lines = self.history_draft.take().unwrap_or_else(|| vec![String::new()]);
                if self.lines.is_empty() {
                    self.lines = vec![String::new()];
                }
                let last = self.lines.len() - 1;
                self.cursor = (last, self.lines[last].len());
            }
            Some(p) => {
                let next_pos = p - 1;
                self.history_pos = Some(next_pos);
                let idx = self.history.len() - 1 - next_pos;
                let text = self.history[idx].clone();
                self.lines = text.split('\n').map(String::from).collect();
                if self.lines.is_empty() {
                    self.lines = vec![String::new()];
                }
                let last = self.lines.len() - 1;
                self.cursor = (last, self.lines[last].len());
            }
        }
    }

    fn take_input(&mut self) -> String {
        let text = self.current_text();
        if !text.trim().is_empty() {
            // Add to history, avoiding consecutive duplicates
            if self.history.back().map(|s| s.as_str()) != Some(&text) {
                self.history.push_back(text.clone());
                if self.history.len() > 200 {
                    self.history.pop_front();
                }
            }
        }
        self.lines = vec![String::new()];
        self.cursor = (0, 0);
        self.history_pos = None;
        self.history_draft = None;
        text
    }

    fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.cursor = (0, 0);
        self.history_pos = None;
    }

    fn line_count(&self) -> usize {
        self.lines.len()
    }
}

// ============================================================================
// APP STATE
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum AppStatus {
    Idle,
    Waiting,  // Sent message, waiting for first event
    Streaming, // Receiving stream chunks
}

struct App {
    messages: Vec<ChatMessage>,
    input: InputState,
    status: AppStatus,
    conversation_id: Option<u64>,
    /// Scroll offset for message pane (lines from bottom)
    scroll_offset: usize,
    /// Whether we're pinned to the bottom (auto-scroll)
    auto_scroll: bool,
    /// Cached rendered line count for scroll math
    rendered_line_count: usize,
    /// Show help overlay
    show_help: bool,
    /// Spinner frame for "waiting" state
    spinner_frame: usize,
    last_spinner_tick: Instant,
}

impl App {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
            input: InputState::new(),
            status: AppStatus::Idle,
            conversation_id: None,
            scroll_offset: 0,
            auto_scroll: true,
            rendered_line_count: 0,
            show_help: false,
            spinner_frame: 0,
            last_spinner_tick: Instant::now(),
        }
    }

    fn add_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
        if self.auto_scroll {
            self.scroll_offset = 0;
        }
    }

    fn append_streaming_chunk(&mut self, chunk: &str) {
        if let Some(last) = self.messages.last_mut() {
            if last.streaming {
                last.content.push_str(chunk);
                if self.auto_scroll {
                    self.scroll_offset = 0;
                }
                return;
            }
        }
        // No streaming message yet — start one
        let mut msg = ChatMessage::assistant_streaming();
        msg.content.push_str(chunk);
        self.messages.push(msg);
        if self.auto_scroll {
            self.scroll_offset = 0;
        }
    }

    fn finalize_streaming(&mut self) {
        if let Some(last) = self.messages.last_mut() {
            if last.streaming {
                last.streaming = false;
            }
        }
    }

    fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
        self.auto_scroll = false;
    }

    fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
        if self.scroll_offset == 0 {
            self.auto_scroll = true;
        }
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    fn tick_spinner(&mut self) {
        if self.last_spinner_tick.elapsed() >= Duration::from_millis(80) {
            self.spinner_frame = (self.spinner_frame + 1) % SPINNER.len();
            self.last_spinner_tick = Instant::now();
        }
    }
}

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ============================================================================
// RENDERING
// ============================================================================

fn render(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Clear background
    frame.render_widget(
        Block::default().style(Style::default().bg(Theme::BG)),
        size,
    );

    // Calculate input height (1 line per input row, min 3, max 12, + 2 for border)
    let input_inner_height = app.input.line_count().clamp(1, 10) as u16;
    let input_height = input_inner_height + 2;
    let status_height = 1u16;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),                      // messages
            Constraint::Length(input_height),        // input
            Constraint::Length(status_height),       // status bar
        ])
        .split(size);

    render_messages(frame, app, chunks[0]);
    render_input(frame, app, chunks[1]);
    render_status(frame, app, chunks[2]);

    if app.show_help {
        render_help(frame, size);
    }
}

fn render_messages(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Theme::BORDER))
        .title(Span::styled(
            " artificer ",
            Style::default().fg(Theme::TEXT_DIM).add_modifier(Modifier::ITALIC),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build all rendered lines
    let mut all_lines: Vec<Line> = Vec::new();

    for msg in &app.messages {
        render_message_lines(msg, &mut all_lines, inner.width as usize);
        // Spacer between messages
        all_lines.push(Line::from(""));
    }

    // Add spinner if waiting/streaming
    if matches!(app.status, AppStatus::Waiting | AppStatus::Streaming) {
        let spinner = SPINNER[app.spinner_frame];
        all_lines.push(Line::from(vec![
            Span::styled(format!("  {} ", spinner), Style::default().fg(Theme::ASSISTANT)),
            Span::styled("thinking...", Style::default().fg(Theme::TEXT_DIM).add_modifier(Modifier::ITALIC)),
        ]));
        all_lines.push(Line::from(""));
    }

    let total_lines = all_lines.len();
    let visible_height = inner.height as usize;

    // Clamp scroll offset
    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll = app.scroll_offset.min(max_scroll);
    let start = total_lines.saturating_sub(visible_height + scroll);

    let visible: Vec<Line> = all_lines
        .into_iter()
        .skip(start)
        .take(visible_height)
        .collect();

    let para = Paragraph::new(visible)
        .style(Style::default().bg(Theme::BG));
    frame.render_widget(para, inner);

    // Scrollbar
    if total_lines > visible_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("█");
        let mut scrollbar_state = ScrollbarState::new(total_lines)
            .position(start);
        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin { vertical: 1, horizontal: 0 }),
            &mut scrollbar_state,
        );
    }
}

fn render_message_lines(msg: &ChatMessage, lines: &mut Vec<Line>, width: usize) {
    match &msg.kind {
        MessageKind::User => {
            lines.push(Line::from(vec![
                Span::styled("  you  ", Style::default()
                    .fg(Theme::BG)
                    .bg(Theme::USER)
                    .add_modifier(Modifier::BOLD)),
                Span::raw("  "),
            ]));
            for line in msg.content.lines() {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(line.to_string(), Style::default().fg(Theme::TEXT_BRIGHT)),
                ]));
            }
        }

        MessageKind::Assistant => {
            lines.push(Line::from(vec![
                Span::styled(" artificer ", Style::default()
                    .fg(Theme::BG)
                    .bg(Theme::ASSISTANT)
                    .add_modifier(Modifier::BOLD)),
                Span::raw("  "),
            ]));
            for line in msg.content.lines() {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(line.to_string(), Style::default().fg(Theme::TEXT)),
                ]));
            }
            if msg.streaming {
                // Blinking cursor effect
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled("▋", Style::default().fg(Theme::ASSISTANT)),
                ]));
            }
        }

        MessageKind::ToolCall { tool, args_preview } => {
            lines.push(Line::from(vec![
                Span::styled(" 🔧 ", Style::default().fg(Theme::TOOL_CALL)),
                Span::styled(tool.clone(), Style::default()
                    .fg(Theme::TOOL_CALL)
                    .add_modifier(Modifier::BOLD)),
                if !args_preview.is_empty() {
                    Span::styled(
                        format!("  {}", args_preview),
                        Style::default().fg(Theme::TEXT_DIM),
                    )
                } else {
                    Span::raw("")
                },
            ]));
            if !msg.content.is_empty() {
                for line in msg.content.lines().take(3) {
                    lines.push(Line::from(vec![
                        Span::styled("     ", Style::default()),
                        Span::styled(line.to_string(), Style::default().fg(Theme::TEXT_DIM)),
                    ]));
                }
            }
        }

        MessageKind::ToolResult { tool: _, truncated } => {
            let preview_lines: Vec<&str> = msg.content.lines().take(4).collect();
            for (i, line) in preview_lines.iter().enumerate() {
                let prefix = if i == 0 { "   ✓ " } else { "     " };
                let style = if i == 0 {
                    Style::default().fg(Theme::TOOL_RESULT)
                } else {
                    Style::default().fg(Theme::TEXT_DIM)
                };
                let truncated_line = if line.len() > width.saturating_sub(8) {
                    format!("{}…", &line[..width.saturating_sub(9).min(line.len())])
                } else {
                    line.to_string()
                };
                lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(truncated_line, style),
                ]));
            }
            if *truncated {
                lines.push(Line::from(vec![
                    Span::styled("     ", Style::default()),
                    Span::styled("[truncated]", Style::default().fg(Theme::TEXT_DIM).add_modifier(Modifier::ITALIC)),
                ]));
            }
        }

        MessageKind::TaskSwitch { from: _, to: _ } => {
            lines.push(Line::from(vec![
                Span::styled(" ⚡ ", Style::default().fg(Theme::TASK_SWITCH)),
                Span::styled(msg.content.clone(), Style::default()
                    .fg(Theme::TASK_SWITCH)
                    .add_modifier(Modifier::ITALIC)),
            ]));
        }

        MessageKind::Error => {
            lines.push(Line::from(vec![
                Span::styled(" ✗  ", Style::default().fg(Theme::ERROR)),
                Span::styled(msg.content.clone(), Style::default().fg(Theme::ERROR)),
            ]));
        }

        MessageKind::System => {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(msg.content.clone(), Style::default()
                    .fg(Theme::TEXT_DIM)
                    .add_modifier(Modifier::ITALIC)),
            ]));
        }
    }
}

fn render_input(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let is_multiline = app.input.line_count() > 1;
    let is_idle = matches!(app.status, AppStatus::Idle);

    let border_style = if is_idle {
        Style::default().fg(Theme::BORDER_ACTIVE)
    } else {
        Style::default().fg(Theme::BORDER)
    };

    let hint = if is_multiline {
        " enter to send · alt+enter for newline "
    } else {
        " enter to send · alt+enter for newline · ↑↓ history · ctrl+h help "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(hint, Style::default().fg(Theme::TEXT_DIM)))
        .style(Style::default().bg(Theme::INPUT_BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build input text with cursor
    let mut lines_out: Vec<Line> = Vec::new();
    let (cursor_row, cursor_col) = app.input.cursor;

    for (row, line_text) in app.input.lines.iter().enumerate() {
        if !is_idle {
            // Greyed out while waiting
            lines_out.push(Line::from(Span::styled(
                line_text.clone(),
                Style::default().fg(Theme::TEXT_DIM),
            )));
            continue;
        }

        if row == cursor_row {
            // Split around cursor
            let before = &line_text[..cursor_col];
            let cursor_char = line_text[cursor_col..].chars().next();
            let after_start = cursor_col + cursor_char.map_or(0, |c| c.len_utf8());
            let after = &line_text[after_start..];

            let cursor_span = match cursor_char {
                Some(c) => Span::styled(
                    c.to_string(),
                    Style::default().fg(Theme::BG).bg(Theme::CURSOR),
                ),
                None => Span::styled(
                    " ",
                    Style::default().fg(Theme::BG).bg(Theme::CURSOR),
                ),
            };

            lines_out.push(Line::from(vec![
                Span::styled(before.to_string(), Style::default().fg(Theme::TEXT_BRIGHT)),
                cursor_span,
                Span::styled(after.to_string(), Style::default().fg(Theme::TEXT_BRIGHT)),
            ]));
        } else {
            lines_out.push(Line::from(Span::styled(
                line_text.clone(),
                Style::default().fg(Theme::TEXT_BRIGHT),
            )));
        }
    }

    let para = Paragraph::new(lines_out)
        .style(Style::default().bg(Theme::INPUT_BG))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, inner);
}

fn render_status(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let conv_str = match app.conversation_id {
        Some(id) => format!(" conv #{} ", id),
        None => " new conversation ".to_string(),
    };

    let (status_str, status_color) = match app.status {
        AppStatus::Idle => ("  ready  ", Theme::STATUS_OK),
        AppStatus::Waiting => ("  waiting  ", Theme::STATUS_BUSY),
        AppStatus::Streaming => ("  streaming  ", Theme::STATUS_BUSY),
    };

    let scroll_indicator = if !app.auto_scroll {
        format!(" ↑ scrolled ({} from bottom) · end to snap ", app.scroll_offset)
    } else {
        String::new()
    };

    let status_line = Line::from(vec![
        Span::styled(
            status_str,
            Style::default().fg(Theme::BG).bg(status_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            conv_str,
            Style::default().fg(Theme::TEXT_DIM).bg(Theme::STATUS_BG),
        ),
        Span::styled(
            scroll_indicator,
            Style::default().fg(Theme::TOOL_CALL).bg(Theme::STATUS_BG),
        ),
        Span::styled(
            "  ctrl+c / ctrl+q to quit",
            Style::default().fg(Theme::TEXT_DIM).bg(Theme::STATUS_BG),
        ),
    ]);

    let para = Paragraph::new(status_line)
        .style(Style::default().bg(Theme::STATUS_BG));
    frame.render_widget(para, area);
}

fn render_help(frame: &mut Frame, area: ratatui::layout::Rect) {
    let width = 54u16;
    let height = 22u16;
    let x = area.width.saturating_sub(width) / 2;
    let y = area.height.saturating_sub(height) / 2;
    let popup_area = ratatui::layout::Rect::new(x, y, width.min(area.width), height.min(area.height));

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Theme::BORDER_ACTIVE))
        .title(Span::styled(" keyboard shortcuts ", Style::default().fg(Theme::TEXT_DIM)))
        .style(Style::default().bg(Theme::STATUS_BG));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let rows: Vec<(&str, &str)> = vec![
        ("Enter", "send message"),
        ("Alt+Enter", "insert newline"),
        ("↑ / ↓", "scroll history (in input)"),
        ("PgUp / PgDn", "scroll messages"),
        ("PgUp / PgDn", "scroll message history"),
        ("Home / End", "cursor to line start/end"),
        ("(while busy) Home/End", "scroll to top/bottom"),
        ("Ctrl+U", "clear input"),
        ("Ctrl+L", "clear screen"),
        ("Ctrl+C / Ctrl+Q", "quit"),
        ("Ctrl+H", "toggle this help"),
        ("", ""),
        ("Paste", "paste works natively"),
        ("", "multi-line paste supported"),
    ];

    let items: Vec<ListItem> = rows
        .iter()
        .map(|(key, desc)| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("  {:20}", key),
                    Style::default().fg(Theme::TOOL_CALL).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    desc.to_string(),
                    Style::default().fg(Theme::TEXT),
                ),
            ]))
        })
        .collect();

    let list = List::new(items).style(Style::default().bg(Theme::STATUS_BG));
    frame.render_widget(list, inner);
}

// ============================================================================
// SHARED STATE FOR ASYNC EVENT INJECTION
// ============================================================================

#[derive(Debug)]
enum UiEvent {
    ChatEvent(ChatEvent),
    RequestComplete,
    RequestError(String),
}

// ============================================================================
// MAIN ENTRY POINTS
// ============================================================================

pub async fn interactive_chat(
    client: ApiClient,
    device_id: i64,
    device_key: String,
) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
    )?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let result = run_app(&mut terminal, client, device_id, device_key).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste,
    )?;
    terminal.show_cursor()?;

    result
}

pub async fn single_message(
    client: ApiClient,
    device_id: i64,
    device_key: String,
    message: String,
) -> Result<()> {
    // For single message mode, use the simple streaming output (no TUI needed)
    println!();
    use artificer_shared::events::ChatEvent;

    match client
        .chat(device_id, device_key, None, message, |event| {
            match event {
                ChatEvent::StreamChunk { content } => {
                    print!("{}", content);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
                ChatEvent::ToolCall { tool, args, .. } => {
                    let preview = extract_args_preview(&args);
                    if preview.is_empty() {
                        eprintln!("\n🔧 {}", tool);
                    } else {
                        eprintln!("\n🔧 {}  {}", tool, preview);
                    }
                }
                ChatEvent::ToolResult { result, truncated, .. } => {
                    let first = result.lines().next().unwrap_or("");
                    if truncated {
                        eprintln!("   ✓ {} [truncated]", first);
                    } else {
                        eprintln!("   ✓ {}", first);
                    }
                }
                ChatEvent::TaskSwitch { from, to } => {
                    eprintln!("⚡ {} → {}", from, to);
                }
                ChatEvent::Error { message } => {
                    eprintln!("\n✗ Error: {}", message);
                }
                _ => {}
            }
        })
        .await
    {
        Ok(_) => println!(),
        Err(e) => eprintln!("Error: {}", e),
    }

    Ok(())
}

// ============================================================================
// APP LOOP
// ============================================================================

async fn run_app(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    client: ApiClient,
    device_id: i64,
    device_key: String,
) -> Result<()> {
    let app = Arc::new(Mutex::new(App::new()));

    // Channel for background tasks to send events to the UI loop
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<UiEvent>();

    // Welcome message
    {
        let mut a = app.lock().unwrap();
        a.add_message(ChatMessage::system(
            "welcome to artificer  ·  ctrl+h for help".to_string(),
        ));
    }

    let tick = Duration::from_millis(50);

    loop {
        // Draw
        {
            let mut a = app.lock().unwrap();
            a.tick_spinner();
            terminal.draw(|f| render(f, &a))?;
        }

        // Drain any incoming chat events (non-blocking)
        loop {
            match event_rx.try_recv() {
                Ok(UiEvent::ChatEvent(chat_event)) => {
                    let mut a = app.lock().unwrap();
                    handle_chat_event(&mut a, chat_event);
                }
                Ok(UiEvent::RequestComplete) => {
                    let mut a = app.lock().unwrap();
                    a.finalize_streaming();
                    a.status = AppStatus::Idle;
                }
                Ok(UiEvent::RequestError(e)) => {
                    let mut a = app.lock().unwrap();
                    a.finalize_streaming();
                    a.status = AppStatus::Idle;
                    a.add_message(ChatMessage::error(e));
                }
                Err(_) => break,
            }
        }

        // Poll for terminal events with timeout
        if event::poll(tick)? {
            let evt = event::read()?;
            let should_quit = handle_terminal_event(
                evt,
                &app,
                &client,
                device_id,
                &device_key,
                event_tx.clone(),
            )
                .await?;
            if should_quit {
                break;
            }
        }
    }

    Ok(())
}

fn handle_chat_event(app: &mut App, event: ChatEvent) {
    match event {
        ChatEvent::StreamChunk { content } => {
            app.status = AppStatus::Streaming;
            app.append_streaming_chunk(&content);
        }

        ChatEvent::ToolCall { task: _, tool, args } => {
            let preview = extract_args_preview(&args);
            app.add_message(ChatMessage::tool_call(tool, preview, String::new()));
        }

        ChatEvent::ToolResult { task: _, tool, result, truncated } => {
            app.add_message(ChatMessage::tool_result(tool, truncated, result));
        }

        ChatEvent::TaskSwitch { from, to } => {
            app.add_message(ChatMessage::task_switch(from, to));
        }

        ChatEvent::Error { message } => {
            app.add_message(ChatMessage::error(message));
        }

        ChatEvent::Done { conversation_id } => {
            app.conversation_id = Some(conversation_id);
        }

        _ => {}
    }
}

fn extract_args_preview(args: &serde_json::Value) -> String {
    if let Some(goal) = args.get("goal").or_else(|| args.get("request")).or_else(|| args.get("task")) {
        if let Some(s) = goal.as_str() {
            let trimmed = s.trim();
            if trimmed.len() > 60 {
                return format!("\"{}…\"", &trimmed[..60]);
            }
            return format!("\"{}\"", trimmed);
        }
    }
    if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
        return format!("({})", path);
    }
    if let Some(query) = args.get("query").and_then(|v| v.as_str()) {
        let q = query.trim();
        if q.len() > 50 {
            return format!("\"{}…\"", &q[..50]);
        }
        return format!("\"{}\"", q);
    }
    String::new()
}

async fn handle_terminal_event(
    evt: Event,
    app: &Arc<Mutex<App>>,
    client: &ApiClient,
    device_id: i64,
    device_key: &str,
    event_tx: tokio::sync::mpsc::UnboundedSender<UiEvent>,
) -> Result<bool> {
    match evt {
        // ----------------------------------------------------------------
        // Bracketed paste — dump straight into input buffer
        // ----------------------------------------------------------------
        Event::Paste(text) => {
            let mut a = app.lock().unwrap();
            if matches!(a.status, AppStatus::Idle) {
                a.input.insert_str(&text);
            }
        }

        // ----------------------------------------------------------------
        // Mouse scroll
        // ----------------------------------------------------------------
        Event::Mouse(MouseEvent { kind, .. }) => {
            let mut a = app.lock().unwrap();
            match kind {
                MouseEventKind::ScrollUp => a.scroll_up(3),
                MouseEventKind::ScrollDown => a.scroll_down(3),
                _ => {}
            }
        }

        // ----------------------------------------------------------------
        // Key events
        // ----------------------------------------------------------------
        Event::Key(key) => {
            return handle_key(key, app, client, device_id, device_key, event_tx).await;
        }

        _ => {}
    }

    Ok(false)
}

async fn handle_key(
    key: KeyEvent,
    app: &Arc<Mutex<App>>,
    client: &ApiClient,
    device_id: i64,
    device_key: &str,
    event_tx: tokio::sync::mpsc::UnboundedSender<UiEvent>,
) -> Result<bool> {
    use KeyCode::*;
    use KeyModifiers as Mod;

    // Global shortcuts that work regardless of state
    match (key.modifiers, key.code) {
        (Mod::CONTROL, Char('c')) | (Mod::CONTROL, Char('q')) => return Ok(true),
        (Mod::CONTROL, Char('h')) => {
            let mut a = app.lock().unwrap();
            a.show_help = !a.show_help;
            return Ok(false);
        }
        _ => {}
    }

    let status = {
        let a = app.lock().unwrap();
        a.status.clone()
    };

    // While waiting/streaming, only allow scroll and quit
    if !matches!(status, AppStatus::Idle) {
        let mut a = app.lock().unwrap();
        match (key.modifiers, key.code) {
            (Mod::NONE, PageUp) => a.scroll_up(10),
            (Mod::NONE, PageDown) => a.scroll_down(10),
            (Mod::NONE, Home) => {
                a.auto_scroll = false;
                a.scroll_offset = usize::MAX;
            }
            (Mod::NONE, End) => a.scroll_to_bottom(),
            _ => {}
        }
        return Ok(false);
    }

    // Idle-only key handling
    let mut a = app.lock().unwrap();

    match (key.modifiers, key.code) {
        // Send message
        (Mod::NONE, Enter) => {
            if a.input.is_empty() {
                return Ok(false);
            }
            let text = a.input.take_input();
            a.add_message(ChatMessage::user(text.clone()));
            a.status = AppStatus::Waiting;
            a.show_help = false;

            let conv_id = a.conversation_id;
            drop(a); // Release lock before spawning

            // Spawn background task for the request
            let client = client.clone();
            let dk = device_key.to_string();
            tokio::spawn(async move {
                match client
                    .chat(device_id, dk, conv_id, text, |evt| {
                        let _ = event_tx.send(UiEvent::ChatEvent(evt));
                    })
                    .await
                {
                    Ok(_) => {
                        let _ = event_tx.send(UiEvent::RequestComplete);
                    }
                    Err(e) => {
                        let _ = event_tx.send(UiEvent::RequestError(e.to_string()));
                    }
                }
            });
            return Ok(false);
        }

        // Newline in input
        (Mod::ALT, Enter) => {
            a.input.insert_newline();
        }

        // Navigation keys in single-line mode trigger history
        (Mod::NONE, Up) if a.input.line_count() == 1 => {
            a.input.history_up();
        }
        (Mod::NONE, Down) if a.input.line_count() == 1 => {
            a.input.history_down();
        }

        // Navigation in multiline input
        (Mod::NONE, Up) => a.input.move_up(),
        (Mod::NONE, Down) => a.input.move_down(),
        (Mod::NONE, Left) => a.input.move_left(),
        (Mod::NONE, Right) => a.input.move_right(),
        (Mod::NONE, Home) => a.input.move_home(),
        (Mod::NONE, End) => a.input.move_end(),

        // Editing
        (Mod::NONE, Backspace) | (Mod::CONTROL, Char('h')) => a.input.backspace(),
        (Mod::NONE, Delete) => a.input.delete(),
        (Mod::CONTROL, Char('u')) => a.input.clear(),

        // Clear screen (rebuild messages display)
        (Mod::CONTROL, Char('l')) => {
            a.messages.retain(|m| matches!(m.kind, MessageKind::User | MessageKind::Assistant));
        }

        // Scroll message pane
        (Mod::NONE, PageUp) => a.scroll_up(10),
        (Mod::NONE, PageDown) => a.scroll_down(10),

        // Character input
        (Mod::NONE | Mod::SHIFT, Char(c)) => {
            a.input.insert_char(c);
        }

        _ => {}
    }

    Ok(false)
}