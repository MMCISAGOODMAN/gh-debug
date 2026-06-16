use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogStream {
    System,
    StepHeader,
    Stdout,
    Stderr,
    Success,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub stream: LogStream,
    pub text: String,
}

impl LogLine {
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            stream: LogStream::System,
            text: text.into(),
        }
    }

    pub fn step_header(text: impl Into<String>) -> Self {
        Self {
            stream: LogStream::StepHeader,
            text: text.into(),
        }
    }

    pub fn stdout(text: impl Into<String>) -> Self {
        Self {
            stream: LogStream::Stdout,
            text: text.into(),
        }
    }

    pub fn stderr(text: impl Into<String>) -> Self {
        Self {
            stream: LogStream::Stderr,
            text: text.into(),
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self {
            stream: LogStream::Success,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            stream: LogStream::Error,
            text: text.into(),
        }
    }

    pub fn to_line(&self) -> Line<'static> {
        match self.stream {
            LogStream::System => Line::from(Span::styled(
                self.text.clone(),
                Style::default().fg(Color::DarkGray),
            )),
            LogStream::StepHeader => Line::from(Span::styled(
                self.text.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            LogStream::Stdout => Line::from(Span::styled(
                self.text.clone(),
                Style::default().fg(Color::White),
            )),
            LogStream::Stderr => Line::from(Span::styled(
                format!("stderr │ {}", self.text),
                Style::default().fg(Color::Red),
            )),
            LogStream::Success => Line::from(Span::styled(
                self.text.clone(),
                Style::default().fg(Color::Green),
            )),
            LogStream::Error => Line::from(Span::styled(
                self.text.clone(),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            )),
        }
    }
}

pub fn push_exec_output(lines: &mut Vec<LogLine>, stdout: &str, stderr: &str) {
    for line in stdout.lines() {
        if !line.is_empty() {
            lines.push(LogLine::stdout(line));
        }
    }
    for line in stderr.lines() {
        if !line.is_empty() {
            lines.push(LogLine::stderr(line));
        }
    }
}

pub fn format_failure(exit_code: i64, stderr: &str, step_name: &str) -> String {
    let tail: Vec<&str> = stderr
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    if tail.is_empty() {
        format!("Step \"{}\" failed (exit code {})", step_name, exit_code)
    } else {
        format!(
            "Step \"{}\" failed (exit code {}): {}",
            step_name,
            exit_code,
            tail.join(" | ")
        )
    }
}
