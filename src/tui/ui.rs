use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

use super::spinner::{dots, format_elapsed, indeterminate_bar, spinner_frame};
use super::DebuggerState;
use crate::session::StepStatus;

pub fn draw_ui(f: &mut Frame, state: &DebuggerState) {
    let mut constraints = vec![Constraint::Length(3)];

    if state.last_error.is_some() {
        constraints.push(Constraint::Length(3));
    }

    if state.running {
        constraints.push(Constraint::Length(3));
    }

    constraints.extend([
        Constraint::Min(8),
        Constraint::Length(3),
    ]);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(f.area());

    let mut idx = 0;

    draw_header(f, state, chunks[idx]);
    idx += 1;

    if state.last_error.is_some() {
        draw_error_banner(f, state, chunks[idx]);
        idx += 1;
    }

    if state.running {
        draw_running_banner(f, state, chunks[idx]);
        idx += 1;
    }

    let main = chunks[idx];
    idx += 1;
    let footer = chunks[idx];

    let main_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(main);

    draw_steps(f, state, main_cols[0]);
    draw_output(f, state, main_cols[1]);
    draw_footer(f, state, footer);
}

fn draw_header(f: &mut Frame, state: &DebuggerState, area: Rect) {
    let (status_icon, status_style) = if state.running {
        let spin = spinner_frame(state.spinner_tick);
        (
            format!("▶ RUNNING {} ", spin),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )
    } else if state.finished {
        ("✓ DONE".to_string(), Style::default().fg(Color::Green))
    } else if state.last_error.is_some() {
        (
            "✗ FAILED".to_string(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        (
            "⏸ PAUSED".to_string(),
            Style::default().fg(Color::Cyan),
        )
    };

    let elapsed = state
        .run_started_at
        .map(|t| format!(" │ {}", format_elapsed(t)))
        .unwrap_or_default();

    let line = Line::from(vec![
        Span::styled(status_icon, status_style),
        Span::styled(elapsed, Style::default().fg(Color::DarkGray)),
        Span::raw("│ "),
        Span::styled(&state.workflow_name, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" │ job: "),
        Span::styled(&state.job_name, Style::default().fg(Color::Cyan)),
        Span::raw(format!(
            " │ step {}/{}",
            state.current_step + 1,
            state.steps.len()
        )),
    ]);

    let block = Block::default().borders(Borders::ALL).title(" gh-debug ");
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn draw_running_banner(f: &mut Frame, state: &DebuggerState, area: Rect) {
    let step_name = state
        .steps
        .get(state.current_step)
        .map(|s| s.name.as_str())
        .unwrap_or("step");

    let elapsed = state
        .run_started_at
        .map(|t| format_elapsed(t))
        .unwrap_or_else(|| "0.0s".into());

    let spin = spinner_frame(state.spinner_tick);
    let bar_width = area.width.saturating_sub(4) as usize / 2;
    let bar = indeterminate_bar(state.spinner_tick, bar_width);
    let dot = dots(state.spinner_tick);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            format!(" {} RUNNING ", spin),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));

    let text = Line::from(vec![
        Span::styled(
            format!("Step {}: ", state.current_step + 1),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(step_name, Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(format!("{} ", dot), Style::default().fg(Color::Yellow)),
        Span::raw("["),
        Span::styled(bar, Style::default().fg(Color::Yellow)),
        Span::raw("] "),
        Span::styled(elapsed, Style::default().fg(Color::DarkGray)),
    ]);

    f.render_widget(Paragraph::new(text).block(block), area);
}

fn draw_error_banner(f: &mut Frame, state: &DebuggerState, area: Rect) {
    let text = state.last_error.as_deref().unwrap_or("");
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(Span::styled(" ERROR ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
    f.render_widget(
        Paragraph::new(Span::styled(
            format!(" ✗ {}", text),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ))
        .block(block),
        area,
    );
}

fn draw_steps(f: &mut Frame, state: &DebuggerState, area: Rect) {
    let items: Vec<ListItem> = state
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let (icon, icon_color) = if step.status == StepStatus::Running {
                (spinner_frame(state.spinner_tick), Color::Yellow)
            } else {
                match step.status {
                    StepStatus::Success => ("✓", Color::Green),
                    StepStatus::Failed => ("✗", Color::Red),
                    StepStatus::Waiting => ("⏸", Color::Cyan),
                    StepStatus::Skipped => ("⊘", Color::DarkGray),
                    StepStatus::Pending => ("·", Color::DarkGray),
                    StepStatus::Running => ("▶", Color::Yellow),
                }
            };

            let bp = if state.breakpoints.iter().any(|b| b == &step.name) {
                " ●"
            } else {
                ""
            };

            let dur = if step.status == StepStatus::Running {
                state
                    .run_started_at
                    .map(|t| format_elapsed(t))
                    .unwrap_or_else(|| "...".into())
            } else {
                step.duration_ms
                    .map(|d| format!("{}ms", d))
                    .unwrap_or_else(|| "-".into())
            };

            let base_style = if i == state.current_step {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{} ", icon), base_style.fg(icon_color)),
                    Span::styled(
                        format!("{:>2}. {}{}", i + 1, truncate(&step.name, 22), bp),
                        base_style,
                    ),
                ]),
                Line::from(Span::styled(
                    format!("      {}", dur),
                    Style::default().fg(if step.status == StepStatus::Running {
                        Color::Yellow
                    } else {
                        Color::DarkGray
                    }),
                )),
            ])
        })
        .collect();

    let title = if state.running {
        let spin = spinner_frame(state.spinner_tick);
        let elapsed = state
            .run_started_at
            .map(|t| format_elapsed(t))
            .unwrap_or_default();
        format!(" Steps {} running {} ", spin, elapsed)
    } else {
        " Steps ".to_string()
    };

    f.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn draw_output(f: &mut Frame, state: &DebuggerState, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize;
    let total_lines = state.log_lines.len();
    let max_scroll = total_lines.saturating_sub(inner_height.max(1));
    let scroll = state.log_scroll.min(max_scroll);

    let mut visible: Vec<Line> = state
        .log_lines
        .iter()
        .skip(scroll)
        .take(inner_height)
        .map(|l| l.to_line())
        .collect();

    if state.running && visible.len() < inner_height {
        let spin = spinner_frame(state.spinner_tick);
        let dot = dots(state.spinner_tick);
        visible.push(Line::from(Span::styled(
            format!("{} waiting for output{} ", spin, dot),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let scroll_hint = if state.auto_scroll { "auto" } else { "manual" };

    let title = if state.running {
        let spin = spinner_frame(state.spinner_tick);
        let elapsed = state
            .run_started_at
            .map(|t| format_elapsed(t))
            .unwrap_or_default();
        format!(
            " Output {} {} │ {} lines, {} ",
            spin, elapsed, total_lines, scroll_hint
        )
    } else {
        format!(
            " Output [{}/{} lines, {}] ",
            scroll + visible.len().min(total_lines.saturating_sub(scroll)),
            total_lines,
            scroll_hint
        )
    };

    f.render_widget(
        Paragraph::new(visible).block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );

    if total_lines > inner_height {
        let mut sb_state = ScrollbarState::new(total_lines).position(scroll);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_symbol("█")
                .track_symbol(Some("│")),
            area,
            &mut sb_state,
        );
    }
}

fn draw_footer(f: &mut Frame, state: &DebuggerState, area: Rect) {
    let status = if state.running {
        let spin = spinner_frame(state.spinner_tick);
        let step_name = state.step_name(state.current_step);
        let elapsed = state
            .run_started_at
            .map(|t| format_elapsed(t))
            .unwrap_or_default();
        format!(
            "{} Running \"{}\"{} ({})",
            spin,
            truncate(&step_name, 30),
            dots(state.spinner_tick),
            elapsed
        )
    } else {
        state.status_message.clone()
    };

    let keys = if state.running {
        "[Q / Ctrl+C / Esc] Cancel step"
    } else {
        "[R] Run  [S] Skip  [I] Shell  [B] Breakpoint  [N] Run to BP  [↑↓/jk] Scroll  [Q] Quit"
    };

    let status_style = if state.running {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let lines = vec![
        Line::from(Span::styled(status, status_style)),
        Line::from(Span::styled(keys, Style::default().fg(Color::DarkGray))),
    ];

    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Controls ")),
        area,
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}
