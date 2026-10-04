use crate::domain::token::Token;
use crate::domain::usage::{format_metric, TokenUsage, UsageSummary};
use crate::tui::views::RenderState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Gauge, Paragraph, Row, Table, Wrap},
    Frame,
};
use std::collections::HashMap;

pub fn render_monitor_tab(f: &mut Frame, area: Rect, state: &RenderState) {
    let sub = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(5)])
        .split(area);

    render_monitor_gauges(f, sub[0], state);
    render_tokens_table(f, sub[1], state.tokens, state.token_usages);
}

fn render_monitor_gauges(f: &mut Frame, area: Rect, state: &RenderState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(area);

    let active_ratio = if state.tokens.is_empty() {
        0.0
    } else {
        state.active_count as f64 / state.tokens.len() as f64
    };
    let g1 = Gauge::default()
        .block(
            Block::default()
                .title(" Token Pool Health ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .gauge_style(Style::default().fg(Color::Green))
        .ratio(active_ratio.clamp(0.0, 1.0))
        .label(format!(
            "{}/{} Active",
            state.active_count,
            state.tokens.len()
        ));
    f.render_widget(g1, chunks[0]);

    let inf_ratio = (state.in_flight_count as f64 / 8.0).clamp(0.0, 1.0);
    let g2 = Gauge::default()
        .block(
            Block::default()
                .title(" In-Flight Requests ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .gauge_style(Style::default().fg(Color::Yellow))
        .ratio(inf_ratio)
        .label(format!("{} In-Flight", state.in_flight_count));
    f.render_widget(g2, chunks[1]);
    render_pow_box(f, chunks[2], state.pow_latency_ms);
}

fn render_pow_box(f: &mut Frame, area: Rect, pow_latency_ms: f64) {
    let pow_str = format!("{:.1}ms", pow_latency_ms);
    let pow_p = Paragraph::new(vec![
        Line::from(vec![
            Span::raw("PoW WASM: "),
            Span::styled(
                &pow_str,
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("Protocol: "),
            Span::styled("DeepSeekHashV1", Style::default().fg(Color::DarkGray)),
        ]),
    ])
    .block(
        Block::default()
            .title(" PoW Solver ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded),
    );
    f.render_widget(pow_p, area);
}

fn build_usage_row(s: &UsageSummary) -> Row<'_> {
    Row::new(vec![
        Cell::from(s.period.clone()),
        Cell::from(format_metric(s.requests, false)),
        Cell::from(format_metric(s.prompt_tokens, false)),
        Cell::from(format_metric(s.cached_tokens, false)).style(Style::default().fg(Color::Cyan)),
        Cell::from(format_metric(s.completion_tokens, false)),
        Cell::from(format_metric(s.total_tokens, false)).style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from(format!("{:.1}%", s.cache_hit_rate())).style(Style::default().fg(Color::Cyan)),
    ])
}

pub fn render_usage_tab(f: &mut Frame, area: Rect, summaries: &[UsageSummary]) {
    let header = Row::new(vec![
        Cell::from("Period").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Requests").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Prompt (In)").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Cached (Save)").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Compl (Out)").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Total Tokens").style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Hit Rate").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .bottom_margin(1);

    let rows: Vec<Row> = summaries.iter().map(build_usage_row).collect();
    let widths = [
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(13),
        Constraint::Length(13),
        Constraint::Length(13),
        Constraint::Length(14),
        Constraint::Min(10),
    ];

    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .title(" 📊 Token Usage Analytics (K, M, B) ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded),
    );
    f.render_widget(table, area);
}

pub fn render_tokens_tab(f: &mut Frame, area: Rect, state: &RenderState) {
    render_tokens_table(f, area, state.tokens, state.token_usages);
}

fn build_token_row<'a>(t: &'a Token, usage: Option<&TokenUsage>) -> Row<'a> {
    let status_style = match t.status.as_str() {
        "ACTIVE" => Style::default().fg(Color::Green),
        "RATE_LIMITED" => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::Red),
    };
    let masked = if t.token.len() > 10 {
        format!("{}...{}", &t.token[..4], &t.token[t.token.len() - 4..])
    } else {
        "***".to_string()
    };
    let (reqs, p, c, tot, hit_rate) = match usage {
        Some(u) => (
            format_metric(u.requests, false),
            format_metric(u.prompt_tokens, false),
            format_metric(u.completion_tokens, false),
            format_metric(u.total_tokens, false),
            format!("{:.1}%", u.cache_hit_rate()),
        ),
        None => (
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0".to_string(),
            "0.0%".to_string(),
        ),
    };

    Row::new(vec![
        Cell::from(t.id.to_string()),
        Cell::from(t.alias.as_deref().unwrap_or("-")),
        Cell::from(t.status.clone()).style(status_style),
        Cell::from(reqs),
        Cell::from(p),
        Cell::from(c),
        Cell::from(tot).style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from(hit_rate).style(Style::default().fg(Color::Cyan)),
        Cell::from(masked),
    ])
}

pub fn render_tokens_table(
    f: &mut Frame,
    area: Rect,
    tokens: &[Token],
    usages: &HashMap<i64, TokenUsage>,
) {
    let header = Row::new(vec![
        Cell::from("ID").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Alias").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Status").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Reqs").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Prompt").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Compl").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Total").style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Hit Rate").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Token (Masked)").style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .bottom_margin(1);

    let rows: Vec<Row> = tokens
        .iter()
        .map(|t| build_token_row(t, usages.get(&t.id)))
        .collect();
    let widths = [
        Constraint::Length(4),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(7),
        Constraint::Length(9),
        Constraint::Length(9),
        Constraint::Length(10),
        Constraint::Length(9),
        Constraint::Min(12),
    ];

    let table = Table::new(rows, widths).header(header).block(
        Block::default()
            .title(" Registered Tokens & Account Usage ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded),
    );
    f.render_widget(table, area);
}

pub fn render_diagnostics_tab(f: &mut Frame, area: Rect, state: &RenderState) {
    let text = vec![
        Line::from(vec![
            Span::styled("Endpoint: ", Style::default().fg(Color::White)),
            Span::styled(state.server_url, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Health:   ", Style::default().fg(Color::White)),
            if state.server_online {
                Span::styled("Healthy (200 OK)", Style::default().fg(Color::Green))
            } else {
                Span::styled("Unreachable", Style::default().fg(Color::Red))
            },
        ]),
        Line::from(vec![
            Span::styled("PoW WASM: ", Style::default().fg(Color::White)),
            Span::styled(
                format!("{:.2}ms execution benchmark", state.pow_latency_ms),
                Style::default().fg(Color::Magenta),
            ),
        ]),
        Line::from(vec![
            Span::styled("Tokens:   ", Style::default().fg(Color::White)),
            Span::styled(
                format!(
                    "{} total ({} active)",
                    state.tokens.len(),
                    state.active_count
                ),
                Style::default().fg(Color::Cyan),
            ),
        ]),
    ];

    let p = Paragraph::new(text)
        .block(
            Block::default()
                .title(" System Diagnostics ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}
