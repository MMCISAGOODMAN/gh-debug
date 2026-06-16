use serde::Serialize;

use crate::session::StepStatus;
use crate::tui::log::LogStream;
use crate::tui::DebuggerState;

#[derive(Serialize, Clone)]
pub struct StateSnapshot {
    pub workflow_name: String,
    pub job_name: String,
    pub current_step: usize,
    pub total_steps: usize,
    pub steps: Vec<StepSnapshot>,
    pub logs: Vec<LogSnapshot>,
    pub status_message: String,
    pub running: bool,
    pub finished: bool,
    pub last_error: Option<String>,
    pub spinner_tick: u64,
    pub elapsed_secs: Option<f64>,
}

#[derive(Serialize, Clone)]
pub struct StepSnapshot {
    pub index: usize,
    pub name: String,
    pub status: &'static str,
    pub duration_ms: Option<u64>,
    pub is_current: bool,
    pub has_breakpoint: bool,
}

#[derive(Serialize, Clone)]
pub struct LogSnapshot {
    pub kind: &'static str,
    pub text: String,
}

pub fn from_debugger_state(state: &DebuggerState) -> StateSnapshot {
    let elapsed_secs = state.run_started_at.map(|t| t.elapsed().as_secs_f64());

    StateSnapshot {
        workflow_name: state.workflow_name.clone(),
        job_name: state.job_name.clone(),
        current_step: state.current_step,
        total_steps: state.steps.len(),
        steps: state
            .steps
            .iter()
            .enumerate()
            .map(|(i, step)| StepSnapshot {
                index: i,
                name: step.name.clone(),
                status: step_status_label(&step.status),
                duration_ms: step.duration_ms,
                is_current: i == state.current_step,
                has_breakpoint: state.breakpoints.iter().any(|b| b == &step.name),
            })
            .collect(),
        logs: state
            .log_lines
            .iter()
            .map(|line| LogSnapshot {
                kind: log_kind_label(&line.stream),
                text: line.text.clone(),
            })
            .collect(),
        status_message: state.status_message.clone(),
        running: state.running,
        finished: state.finished,
        last_error: state.last_error.clone(),
        spinner_tick: state.spinner_tick,
        elapsed_secs,
    }
}

fn step_status_label(status: &StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "pending",
        StepStatus::Waiting => "waiting",
        StepStatus::Running => "running",
        StepStatus::Success => "success",
        StepStatus::Failed => "failed",
        StepStatus::Skipped => "skipped",
    }
}

fn log_kind_label(stream: &LogStream) -> &'static str {
    match stream {
        LogStream::System => "system",
        LogStream::StepHeader => "header",
        LogStream::Stdout => "stdout",
        LogStream::Stderr => "stderr",
        LogStream::Success => "success",
        LogStream::Error => "error",
    }
}
