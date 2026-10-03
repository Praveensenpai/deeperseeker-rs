use crate::domain::token::Token;
use crate::domain::usage::UsageSummary;
use crate::tui::tabs::{
    render_diagnostics_tab, render_monitor_tab, render_tokens_tab, render_usage_tab,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Monitor,
    Usage,
    Tokens,
    Diagnostics,
}

impl ActiveTab {
    pub fn next(self) -> Self {
        match self {
            ActiveTab::Monitor => ActiveTab::Usage,
            ActiveTab::Usage => ActiveTab::Tokens,
            ActiveTab::Tokens => ActiveTab::Diagnostics,
            ActiveTab::Diagnostics => ActiveTab::Monitor,
        }
    }
}

pub struct RenderState<'a> {
    pub server_online: bool,
    pub server_version: &'a str,
    pub server_url: &'a str,
    pub active_tab: ActiveTab,
    pub tokens: &'a [Token],
    pub summaries: &'a [UsageSummary],
    pub active_count: usize,
    pub in_flight_count: usize,
    pub pow_latency_ms: f64,
    pub is_paused: bool,
    pub last_updated: &'a str,
}

pub fn render_ui(f: &mut Frame, state: &RenderState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(f.size());

    render_header(f, chunks[0], state);
    render_tabs(f, chunks[1], state.active_tab);

    match state.active_tab {
        ActiveTab::Monitor => render_monitor_tab(f, chunks[2], state),
        ActiveTab::Usage => render_usage_tab(f, chunks[2], state.summaries),
        ActiveTab::Tokens => render_tokens_tab(f, chunks[2], state),
        ActiveTab::Diagnostics => render_diagnostics_tab(f, chunks[2], state),
    }

    render_footer(f, chunks[3], state);
}

fn render_header(f: &mut Frame, area: Rect, state: &RenderState) {
    let status_badge = if state.server_online {
        Span::styled(
            " ● ONLINE ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " ○ OFFLINE ",
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )
    };

    let title_line = Line::from(vec![
        Span::styled(
            " 深索 deeperseeker ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("v{} ", state.server_version),
            Style::default().fg(Color::DarkGray),
        ),
        status_badge,
        Span::raw("  Gateway: "),
        Span::styled(state.server_url, Style::default().fg(Color::Yellow)),
        Span::raw("  Updated: "),
        Span::styled(state.last_updated, Style::default().fg(Color::DarkGray)),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));
    let p = Paragraph::new(title_line)
        .block(block)
        .alignment(Alignment::Left);
    f.render_widget(p, area);
}

fn render_tabs(f: &mut Frame, area: Rect, current: ActiveTab) {
    let make_tab = |label: &str, tab: ActiveTab| {
        if tab == current {
            Span::styled(
                format!(" [ {} ] ", label),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(format!("   {}   ", label), Style::default().fg(Color::Gray))
        }
    };

    let line = Line::from(vec![
        make_tab("1. Monitor", ActiveTab::Monitor),
        Span::raw(" "),
        make_tab("2. Token Analytics", ActiveTab::Usage),
        Span::raw(" "),
        make_tab("3. Tokens", ActiveTab::Tokens),
        Span::raw(" "),
        make_tab("4. Diagnostics", ActiveTab::Diagnostics),
    ]);

    let block = Block::default().borders(Borders::BOTTOM);
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn render_footer(f: &mut Frame, area: Rect, state: &RenderState) {
    let pause_badge = if state.is_paused {
        Span::styled(
            " [PAUSED] ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " [LIVE] ",
            Style::default().fg(Color::Black).bg(Color::Green),
        )
    };

    let line = Line::from(vec![
        pause_badge,
        Span::raw("  "),
        Span::styled(
            "Tab",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Switch Tab  "),
        Span::styled(
            "r",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Refresh  "),
        Span::styled(
            "p",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Pause/Resume  "),
        Span::styled(
            "q / Esc",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(": Quit"),
    ]);

    let block = Block::default().borders(Borders::TOP);
    f.render_widget(Paragraph::new(line).block(block), area);
}
