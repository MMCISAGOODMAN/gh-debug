use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugSession {
    pub id: String,
    pub workflow_path: PathBuf,
    pub job_name: String,
    pub current_step: usize,
    pub breakpoints: Vec<String>,
    pub step_states: Vec<StepState>,
    pub container_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub events: Vec<SessionEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Waiting,
    Running,
    Success,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepState {
    pub name: String,
    pub status: StepStatus,
    pub duration_ms: Option<u64>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    Start { step_index: usize, step_name: String },
    Complete {
        step_index: usize,
        success: bool,
        duration_ms: u64,
    },
    Skip { step_index: usize },
    BreakpointSet { step_name: String, enabled: bool },
    ShellEnter { step_index: usize },
    Pause { step_index: usize, reason: String },
    Quit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureSnapshot {
    pub container_id: String,
    pub step_index: usize,
    pub step_name: String,
    pub logs: String,
    pub saved_at: DateTime<Utc>,
}

pub struct SessionStore;

impl SessionStore {
    pub fn dir() -> PathBuf {
        PathBuf::from(".gh-debug")
    }

    pub fn session_path() -> PathBuf {
        Self::dir().join("session.json")
    }

    pub fn recording_path() -> PathBuf {
        Self::dir().join("recording.json")
    }

    pub fn failure_path() -> PathBuf {
        Self::dir().join("last-failure.json")
    }

    pub fn ensure_dir() -> Result<()> {
        fs::create_dir_all(Self::dir()).context("create .gh-debug directory")
    }

    pub fn save_session(session: &DebugSession) -> Result<()> {
        Self::ensure_dir()?;
        let json = serde_json::to_string_pretty(session)?;
        fs::write(Self::session_path(), json)?;
        Ok(())
    }

    pub fn save_recording(session: &DebugSession) -> Result<()> {
        Self::ensure_dir()?;
        let json = serde_json::to_string_pretty(session)?;
        fs::write(Self::recording_path(), json)?;
        Ok(())
    }

    pub fn save_failure(snapshot: &FailureSnapshot) -> Result<()> {
        Self::ensure_dir()?;
        let json = serde_json::to_string_pretty(snapshot)?;
        fs::write(Self::failure_path(), json)?;
        Ok(())
    }

    pub fn load_failure() -> Result<FailureSnapshot> {
        let path = Self::failure_path();
        let content = fs::read_to_string(&path)
            .with_context(|| format!("read failure snapshot at {}", path.display()))?;
        serde_json::from_str(&content).context("parse last-failure.json")
    }
}

pub fn load_session() -> Result<DebugSession> {
    let path = SessionStore::session_path();
    let content = fs::read_to_string(&path)
        .with_context(|| format!("read session at {}", path.display()))?;
    serde_json::from_str(&content).context("parse session.json")
}

pub async fn replay_session(path: &Path) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("read recording {}", path.display()))?;
    let session: DebugSession = serde_json::from_str(&content).context("parse recording")?;

    println!("Replaying session {} (job: {})", session.id, session.job_name);
    println!("Workflow: {}", session.workflow_path.display());
    println!();

    for event in &session.events {
        match event {
            SessionEvent::Start { step_index, step_name } => {
                println!("[{}] ▶ Start: {}", step_index, step_name);
            }
            SessionEvent::Complete {
                step_index,
                success,
                duration_ms,
            } => {
                let icon = if *success { "✓" } else { "✗" };
                println!("[{}] {} Done ({}ms)", step_index, icon, duration_ms);
            }
            SessionEvent::Skip { step_index } => {
                println!("[{}] ⊘ Skipped", step_index);
            }
            SessionEvent::BreakpointSet { step_name, enabled } => {
                println!(
                    "Breakpoint {}: {}",
                    if *enabled { "set" } else { "cleared" },
                    step_name
                );
            }
            SessionEvent::ShellEnter { step_index } => {
                println!("[{}] Shell entered", step_index);
            }
            SessionEvent::Pause { step_index, reason } => {
                println!("[{}] ⏸ Paused: {}", step_index, reason);
            }
            SessionEvent::Quit => println!("Session ended"),
        }
    }

    println!();
    println!("Final step states:");
    for (i, state) in session.step_states.iter().enumerate() {
        let icon = match state.status {
            StepStatus::Success => "✓",
            StepStatus::Failed => "✗",
            StepStatus::Skipped => "⊘",
            StepStatus::Running => "▶",
            StepStatus::Waiting => "⏸",
            StepStatus::Pending => "·",
        };
        let dur = state
            .duration_ms
            .map(|d| format!("{}ms", d))
            .unwrap_or_else(|| "-".into());
        println!("  [{}] {} {} ({})", i, icon, state.name, dur);
    }

    Ok(())
}
