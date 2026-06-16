pub mod log;
mod spinner;
mod ui;

use log::{format_failure, push_exec_output, LogLine};
use ui::draw_ui;

use anyhow::{Context, Result};
use chrono::Utc;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::collections::BTreeMap;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::actions::resolve_action;
use crate::docker::{build_run_command, runs_on_to_image, ContainerRunner, ExecEvent, StreamKind};
use crate::env::{inject_secrets, step_env, EnvConfig};
use crate::session::{
    DebugSession, FailureSnapshot, SessionEvent, SessionStore, StepState, StepStatus,
};
use crate::workflow::{default_workflow_dir, Workflow};
use crate::web::{start as start_web, WebBridge, WebConfig};

pub enum DebugAction {
    RunStep,
    SkipStep,
    EnterShell,
    ToggleBreakpoint,
    RunToBreakpoint,
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
    ScrollTop,
    ScrollBottom,
    Cancel,
    Quit,
    None,
}

enum StepOutcome {
    Success,
    Failed,
    Cancelled,
}

pub struct DebuggerState {
    pub workflow_name: String,
    pub job_name: String,
    pub steps: Vec<StepState>,
    pub current_step: usize,
    pub breakpoints: Vec<String>,
    pub status_message: String,
    pub auto_mode: bool,
    pub finished: bool,
    pub running: bool,
    pub spinner_tick: u64,
    pub run_started_at: Option<Instant>,
    pub log_lines: Vec<LogLine>,
    pub log_scroll: usize,
    pub auto_scroll: bool,
    pub last_error: Option<String>,
}

impl DebuggerState {
    pub(crate) fn step_name(&self, index: usize) -> String {
        self.steps
            .get(index)
            .map(|s| s.name.clone())
            .unwrap_or_default()
    }

    fn has_breakpoint(&self, name: &str) -> bool {
        self.breakpoints.iter().any(|b| b == name)
    }

    fn set_waiting(&mut self, index: usize, reason: &str) {
        if let Some(step) = self.steps.get_mut(index) {
            step.status = StepStatus::Waiting;
        }
        self.current_step = index;
        self.running = false;
        self.run_started_at = None;
        self.status_message = reason.to_string();
    }

    fn push_log(&mut self, line: LogLine) {
        self.log_lines.push(line);
        if self.auto_scroll {
            self.log_scroll = self.log_lines.len().saturating_sub(1);
        }
    }

    fn scroll_up(&mut self, amount: usize) {
        self.auto_scroll = false;
        self.log_scroll = self.log_scroll.saturating_sub(amount);
    }

    fn scroll_down(&mut self, amount: usize, max: usize) {
        self.log_scroll = (self.log_scroll + amount).min(max);
        if self.log_scroll >= max.saturating_sub(1) {
            self.auto_scroll = true;
        }
    }

    fn scroll_top(&mut self) {
        self.auto_scroll = false;
        self.log_scroll = 0;
    }

    fn scroll_bottom(&mut self) {
        self.auto_scroll = true;
    }
}

pub async fn run_debugger(
    workflow: Workflow,
    workflow_path: PathBuf,
    job_name: Option<String>,
    breakpoints: Vec<String>,
    resume_session: Option<DebugSession>,
    web_config: WebConfig,
) -> Result<()> {
    let (job_name, job) = workflow.resolve_job(job_name)?;
    let project_root = default_workflow_dir(&workflow_path);
    let env_config = EnvConfig::load_from_project(&project_root)?;

    let mut base_env = env_config.merged_env(job.env.as_ref());
    inject_secrets(&mut base_env, &env_config.secrets);
    let wf_name = workflow.name.clone().unwrap_or_else(|| "workflow".into());
    for (k, v) in env_config.github_env_vars(&wf_name, &job_name) {
        base_env.insert(k, v);
    }
    let mut runtime_env = BTreeMap::<String, String>::new();

    let runs_on = job.runs_on.primary();
    let image = runs_on_to_image(&runs_on);
    let mut runner = ContainerRunner::connect()?.with_workspace(project_root.clone());

    runner
        .create_job_container(image, &base_env)
        .await
        .context("create job container")?;

    let step_names: Vec<String> = job
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| s.display_name(i))
        .collect();

    let (current_step, step_states, mut bp, mut events) = if let Some(session) = resume_session {
        (
            session.current_step,
            session.step_states,
            session.breakpoints,
            session.events,
        )
    } else {
        let states = step_names
            .iter()
            .map(|name| StepState {
                name: name.clone(),
                status: StepStatus::Pending,
                duration_ms: None,
                output: None,
            })
            .collect();
        (0, states, breakpoints.clone(), vec![])
    };

    for bp_name in &breakpoints {
        if !bp.contains(bp_name) {
            bp.push(bp_name.clone());
        }
    }

    let session_id = Uuid::new_v4().to_string();
    let mut state = DebuggerState {
        workflow_name: wf_name.clone(),
        job_name: job_name.clone(),
        steps: step_states,
        current_step,
        breakpoints: bp,
        status_message: "Ready — press R to run current step".into(),
        auto_mode: false,
        finished: false,
        running: false,
        spinner_tick: 0,
        run_started_at: None,
        log_lines: vec![LogLine::system("gh-debug ready")],
        log_scroll: 0,
        auto_scroll: true,
        last_error: None,
    };

    if state.has_breakpoint(&state.step_name(state.current_step)) {
        state.set_waiting(state.current_step, "Breakpoint hit — press R to continue");
        events.push(SessionEvent::Pause {
            step_index: state.current_step,
            reason: "breakpoint".into(),
        });
    } else {
        state.set_waiting(state.current_step, "Ready — press R to run current step");
    }

    let mut debug_session = DebugSession {
        id: session_id,
        workflow_path: workflow_path.clone(),
        job_name: job_name.clone(),
        current_step: state.current_step,
        breakpoints: state.breakpoints.clone(),
        step_states: state.steps.clone(),
        container_id: runner.container_id().map(str::to_string),
        started_at: Utc::now(),
        events: events.clone(),
    };
    SessionStore::save_session(&debug_session)?;

    let mut web_bridge = if web_config.enabled {
        Some(start_web(&web_config).await?)
    } else {
        None
    };

    let mut terminal = if !web_config.web_only {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        Some(ratatui::Terminal::new(backend)?)
    } else {
        eprintln!("[gh-debug] Web-only mode — use the browser to control debugging");
        None
    };
    let _terminal_guard = if terminal.is_some() {
        Some(TerminalGuard)
    } else {
        None
    };

    if let Some(ref web) = web_bridge {
        web.publish(&state);
    }

    let mut should_quit = false;

    loop {
        if let Some(ref mut term) = terminal {
            term.draw(|f| draw_ui(f, &state))?;
        }
        if let Some(ref web) = web_bridge {
            web.publish(&state);
        }

        if state.finished {
            if web_bridge.is_some() {
                tokio::time::sleep(Duration::from_secs(5)).await;
            } else {
                std::thread::sleep(std::time::Duration::from_millis(800));
            }
            break;
        }

        let mut action = None;
        if let Some(ref mut web) = web_bridge {
            action = web.poll_action();
        }
        if action.is_none() {
            if terminal.is_some() {
                if event::poll(std::time::Duration::from_millis(100))? {
                    if let Event::Key(key) = event::read()? {
                        action = Some(map_key(key));
                    }
                }
            } else {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        if let Some(action) = action {
                match action {
                    DebugAction::Quit => {
                        events.push(SessionEvent::Quit);
                        should_quit = true;
                        break;
                    }
                    DebugAction::ScrollUp => state.scroll_up(1),
                    DebugAction::ScrollDown => {
                        let max = state.log_lines.len().saturating_sub(1);
                        state.scroll_down(1, max);
                    }
                    DebugAction::ScrollPageUp => state.scroll_up(10),
                    DebugAction::ScrollPageDown => {
                        let max = state.log_lines.len().saturating_sub(1);
                        state.scroll_down(10, max);
                    }
                    DebugAction::ScrollTop => state.scroll_top(),
                    DebugAction::ScrollBottom => {
                        state.scroll_bottom();
                        state.log_scroll = state.log_lines.len().saturating_sub(1);
                    }
                    DebugAction::RunStep if !state.running => {
                        let step_idx = state.current_step;
                        if matches!(
                            state.steps[step_idx].status,
                            StepStatus::Waiting | StepStatus::Pending
                        ) {
                            state.auto_mode = false;
                            state.last_error = None;
                            let step = job.steps[step_idx].clone();
                            let visible = log_visible_rows(terminal.as_ref());
                            match execute_step(
                                &mut terminal,
                                &mut state,
                                &step,
                                step_idx,
                                &runner,
                                &base_env,
                                &mut runtime_env,
                                &mut events,
                                web_bridge.as_ref(),
                                visible,
                            )
                            .await?
                            {
                                StepOutcome::Success => {
                                    if step_idx + 1 >= state.steps.len() {
                                        state.finished = true;
                                        state.status_message = "All steps completed".into();
                                        state.push_log(LogLine::success("All steps completed"));
                                    } else {
                                        state.current_step = step_idx + 1;
                                        if state.has_breakpoint(&state.step_name(state.current_step)) {
                                            state.set_waiting(
                                                state.current_step,
                                                "Breakpoint hit — press R to continue",
                                            );
                                            events.push(SessionEvent::Pause {
                                                step_index: state.current_step,
                                                reason: "breakpoint".into(),
                                            });
                                        } else {
                                            state.set_waiting(
                                                state.current_step,
                                                "Ready — press R to run current step",
                                            );
                                        }
                                    }
                                }
                                StepOutcome::Failed => {
                                    save_failure(&state, &runner, step_idx).ok();
                                    state.set_waiting(step_idx, "FAILED — press I for shell, R to retry");
                                    events.push(SessionEvent::Pause {
                                        step_index: step_idx,
                                        reason: "failure".into(),
                                    });
                                }
                                StepOutcome::Cancelled => {
                                    state.set_waiting(step_idx, "Cancelled — press R to retry, Q to quit");
                                    events.push(SessionEvent::Pause {
                                        step_index: step_idx,
                                        reason: "cancelled".into(),
                                    });
                                }
                            }
                        }
                    }
                    DebugAction::SkipStep if !state.running => {
                        let idx = state.current_step;
                        state.steps[idx].status = StepStatus::Skipped;
                        events.push(SessionEvent::Skip { step_index: idx });
                        state.push_log(LogLine::system(format!("Skipped step {}: {}", idx + 1, state.step_name(idx))));
                        if idx + 1 >= state.steps.len() {
                            state.finished = true;
                            state.status_message = "All steps completed (with skips)".into();
                        } else {
                            state.current_step = idx + 1;
                            state.set_waiting(state.current_step, "Ready — press R to run current step");
                        }
                    }
                    DebugAction::EnterShell if !state.running => {
                        if let Some(ref mut term) = terminal {
                            disable_raw_mode()?;
                            execute!(term.backend_mut(), LeaveAlternateScreen)?;
                            term.show_cursor()?;
                        }
                        events.push(SessionEvent::ShellEnter {
                            step_index: state.current_step,
                        });
                        match runner.interactive_shell().await {
                            Ok(()) => state.push_log(LogLine::system("Returned from container shell")),
                            Err(e) => state.push_log(LogLine::error(format!("Shell error: {}", e))),
                        }
                        if let Some(ref mut term) = terminal {
                            enable_raw_mode()?;
                            execute!(term.backend_mut(), EnterAlternateScreen)?;
                        }
                    }
                    DebugAction::ToggleBreakpoint if !state.running => {
                        let name = state.step_name(state.current_step);
                        if state.has_breakpoint(&name) {
                            state.breakpoints.retain(|b| b != &name);
                            events.push(SessionEvent::BreakpointSet {
                                step_name: name.clone(),
                                enabled: false,
                            });
                            state.push_log(LogLine::system(format!("Breakpoint cleared: {}", name)));
                        } else {
                            state.breakpoints.push(name.clone());
                            events.push(SessionEvent::BreakpointSet {
                                step_name: name.clone(),
                                enabled: true,
                            });
                            state.push_log(LogLine::system(format!("Breakpoint set: {}", name)));
                        }
                    }
                    DebugAction::RunToBreakpoint if !state.running => {
                        state.auto_mode = true;
                        loop {
                            let step_idx = state.current_step;
                            if matches!(
                                state.steps[step_idx].status,
                                StepStatus::Waiting | StepStatus::Pending
                            ) {
                                state.last_error = None;
                                let step = job.steps[step_idx].clone();
                                let visible = log_visible_rows(terminal.as_ref());
                                match execute_step(
                                    &mut terminal,
                                    &mut state,
                                    &step,
                                    step_idx,
                                    &runner,
                                    &base_env,
                                    &mut runtime_env,
                                    &mut events,
                                    web_bridge.as_ref(),
                                    visible,
                                )
                                .await?
                                {
                                    StepOutcome::Success => {}
                                    StepOutcome::Failed => {
                                        save_failure(&state, &runner, step_idx).ok();
                                        state.set_waiting(step_idx, "FAILED — paused at failure");
                                        break;
                                    }
                                    StepOutcome::Cancelled => {
                                        state.set_waiting(step_idx, "Cancelled — press R to retry, Q to quit");
                                        break;
                                    }
                                }
                            }

                            if state.current_step + 1 >= state.steps.len() {
                                state.finished = true;
                                state.status_message = "All steps completed".into();
                                break;
                            }

                            state.current_step += 1;
                            if state.has_breakpoint(&state.step_name(state.current_step)) {
                                state.set_waiting(
                                    state.current_step,
                                    "Breakpoint hit — press R to continue",
                                );
                                events.push(SessionEvent::Pause {
                                    step_index: state.current_step,
                                    reason: "breakpoint".into(),
                                });
                                break;
                            }
                        }
                    }
                    DebugAction::RunStep | DebugAction::SkipStep | DebugAction::EnterShell
                    | DebugAction::ToggleBreakpoint | DebugAction::RunToBreakpoint | DebugAction::Cancel => {}
                    DebugAction::None => {}
                }

                debug_session.current_step = state.current_step;
                debug_session.breakpoints = state.breakpoints.clone();
                debug_session.step_states = state.steps.clone();
                debug_session.container_id = runner.container_id().map(str::to_string);
                debug_session.events = events.clone();
                SessionStore::save_session(&debug_session)?;

                if state.finished {
                    if let Some(ref web) = web_bridge {
                        web.publish(&state);
                    }
                }
        }
    }

    if let Some(ref mut term) = terminal {
        disable_raw_mode()?;
        execute!(term.backend_mut(), LeaveAlternateScreen)?;
        term.show_cursor()?;
    }

    debug_session.events = events;
    debug_session.step_states = state.steps.clone();
    SessionStore::save_recording(&debug_session)?;

    if should_quit {
        runner.cleanup().await?;
    } else if !state.finished {
        eprintln!("Session saved. Resume with: gh-debug run --resume");
    } else {
        runner.cleanup().await?;
    }

    Ok(())
}

fn log_visible_rows(terminal: Option<&Terminal<CrosstermBackend<Stdout>>>) -> usize {
    terminal
        .and_then(|t| t.size().ok())
        .map(|s| s.height.saturating_sub(12) as usize)
        .unwrap_or(24)
}

fn map_key(key: KeyEvent) -> DebugAction {
    match key.code {
        KeyCode::Char('r') | KeyCode::Char('R') => DebugAction::RunStep,
        KeyCode::Char('s') | KeyCode::Char('S') => DebugAction::SkipStep,
        KeyCode::Char('i') | KeyCode::Char('I') => DebugAction::EnterShell,
        KeyCode::Char('b') | KeyCode::Char('B') => DebugAction::ToggleBreakpoint,
        KeyCode::Char('n') | KeyCode::Char('N') => DebugAction::RunToBreakpoint,
        KeyCode::Char('q') | KeyCode::Char('Q') => DebugAction::Quit,
        KeyCode::Esc => DebugAction::Cancel,
        KeyCode::Up | KeyCode::Char('k') => DebugAction::ScrollUp,
        KeyCode::Down | KeyCode::Char('j') => DebugAction::ScrollDown,
        KeyCode::Home | KeyCode::Char('g') => DebugAction::ScrollTop,
        KeyCode::End | KeyCode::Char('G') => DebugAction::ScrollBottom,
        KeyCode::PageUp => DebugAction::ScrollPageUp,
        KeyCode::PageDown => DebugAction::ScrollPageDown,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => DebugAction::Cancel,
        _ => DebugAction::None,
    }
}

fn is_cancel_key(key: KeyEvent) -> bool {
    matches!(map_key(key), DebugAction::Cancel)
}

fn poll_cancel_key(cancel: &Arc<AtomicBool>) {
    if event::poll(Duration::from_millis(0)).unwrap_or(false) {
        if let Ok(Event::Key(key)) = event::read() {
            if is_cancel_key(key) {
                cancel.store(true, Ordering::Relaxed);
            } else if matches!(map_key(key), DebugAction::Quit) {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn handle_exec_event(
    event: ExecEvent<'_>,
    cancel_flag: &Arc<AtomicBool>,
    state: &mut DebuggerState,
    terminal: &mut Option<Terminal<CrosstermBackend<Stdout>>>,
    web: Option<&WebBridge>,
    visible_rows: usize,
) {
    match event {
        ExecEvent::Tick => {
            poll_cancel_key(cancel_flag);
            state.spinner_tick = state.spinner_tick.wrapping_add(1);
            let _ = redraw(terminal, state, web);
        }
        ExecEvent::Line(stream, text) => {
            let line = match stream {
                StreamKind::Stdout => LogLine::stdout(text),
                StreamKind::Stderr => LogLine::stderr(text),
            };
            state.push_log(line);
            let max_scroll = state.log_lines.len().saturating_sub(visible_rows.max(1));
            if state.auto_scroll {
                state.log_scroll = max_scroll;
            }
            let _ = redraw(terminal, state, web);
        }
    }
}

async fn execute_step(
    terminal: &mut Option<Terminal<CrosstermBackend<Stdout>>>,
    state: &mut DebuggerState,
    step: &crate::workflow::Step,
    index: usize,
    runner: &ContainerRunner,
    base_env: &BTreeMap<String, String>,
    runtime_env: &mut BTreeMap<String, String>,
    events: &mut Vec<SessionEvent>,
    web: Option<&WebBridge>,
    visible_rows: usize,
) -> Result<StepOutcome> {
    let step_name = state.step_name(index);
    events.push(SessionEvent::Start {
        step_index: index,
        step_name: step_name.clone(),
    });

    state.steps[index].status = StepStatus::Running;
    state.running = true;
    state.spinner_tick = 0;
    state.run_started_at = Some(Instant::now());
    state.status_message = format!("Running step {}...", index + 1);
    state.push_log(LogLine::step_header(format!(
        "── Step {}: {} ──",
        index + 1,
        step_name
    )));
    redraw(terminal, state, web)?;

    let mut env = merged_step_env(base_env, runtime_env, step.env.as_ref());
    let start = Instant::now();
    let mut exit_code: i64 = 0;
    let mut combined_output = String::new();
    let mut combined_stderr = String::new();
    let mut success = true;
    let mut cancelled = false;
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();

    if let Some(uses) = &step.uses {
        let action = resolve_action(uses, step.with.as_ref())?;
        state.push_log(LogLine::system(action.description.clone()));

        for (k, v) in &action.env_exports {
            runtime_env.insert(k.clone(), v.clone());
            env.insert(k.clone(), v.clone());
        }
        if !action.env_exports.is_empty() {
            let vars: Vec<_> = action.env_exports.keys().cloned().collect();
            state.push_log(LogLine::system(format!("Exported env: {}", vars.join(", "))));
        }

        if action.skip_container_exec {
            state.push_log(LogLine::system("Skipped (no container exec needed)"));
        } else {
            for cmd in &action.commands {
                let result = runner
                    .exec_command(
                        cmd,
                        &env,
                        |event| {
                            handle_exec_event(
                                event,
                                &cancel_flag,
                                state,
                                terminal,
                                web,
                                visible_rows,
                            )
                        },
                        cancel.clone(),
                    )
                    .await?;
                combined_output.push_str(&result.stdout);
                combined_stderr.push_str(&result.stderr);
                exit_code = result.exit_code;
                if result.cancelled {
                    cancelled = true;
                    break;
                }
                if exit_code != 0 {
                    success = false;
                    break;
                }
            }
        }
    } else if let Some(run) = &step.run {
        let cmd = build_run_command(run, step.shell.as_deref());
        let result = runner
            .exec_command(
                &cmd,
                &env,
                |event| {
                    handle_exec_event(
                        event,
                        &cancel_flag,
                        state,
                        terminal,
                        web,
                        visible_rows,
                    )
                },
                cancel.clone(),
            )
            .await?;
        combined_output.push_str(&result.stdout);
        combined_stderr.push_str(&result.stderr);
        exit_code = result.exit_code;
        cancelled = result.cancelled;
        success = exit_code == 0 && !cancelled;
    } else {
        state.push_log(LogLine::system("(empty step)"));
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    let full_output = format!("{}\n{}", combined_output, combined_stderr);
    state.steps[index].duration_ms = Some(duration_ms);
    state.steps[index].output = Some(full_output);

    if cancelled {
        state.steps[index].status = StepStatus::Waiting;
        state.push_log(LogLine::error(format!(
            "⊘ Step {} cancelled by user ({}ms)",
            index + 1,
            duration_ms
        )));
        state.last_error = None;
        state.running = false;
        state.run_started_at = None;
        events.push(SessionEvent::Complete {
            step_index: index,
            success: false,
            duration_ms,
        });
        redraw(terminal, state, web)?;
        return Ok(StepOutcome::Cancelled);
    }

    if success {
        state.steps[index].status = StepStatus::Success;
        state.push_log(LogLine::success(format!(
            "✓ Step {} passed ({}ms)",
            index + 1,
            duration_ms
        )));
        state.last_error = None;
    } else {
        state.steps[index].status = StepStatus::Failed;
        state.push_log(LogLine::error(format!(
            "✗ Step {} failed — exit code {} ({}ms)",
            index + 1, exit_code, duration_ms
        )));
        if !combined_stderr.trim().is_empty() {
            push_exec_output(&mut state.log_lines, "", &combined_stderr);
        }
        state.last_error = Some(format_failure(exit_code, &combined_stderr, &step_name));
    }

    state.running = false;
    state.run_started_at = None;
    events.push(SessionEvent::Complete {
        step_index: index,
        success,
        duration_ms,
    });
    redraw(terminal, state, web)?;

    Ok(if success {
        StepOutcome::Success
    } else {
        StepOutcome::Failed
    })
}

fn redraw(
    terminal: &mut Option<Terminal<CrosstermBackend<Stdout>>>,
    state: &DebuggerState,
    web: Option<&WebBridge>,
) -> Result<()> {
    if let Some(term) = terminal.as_mut() {
        term.draw(|f| draw_ui(f, state))?;
    }
    if let Some(w) = web {
        w.publish(state);
    }
    Ok(())
}

fn merged_step_env(
    base: &BTreeMap<String, String>,
    runtime: &BTreeMap<String, String>,
    step_env_map: Option<&BTreeMap<String, serde_yaml::Value>>,
) -> BTreeMap<String, String> {
    let mut env = step_env(base, step_env_map);
    for (k, v) in runtime {
        env.insert(k.clone(), v.clone());
    }
    env
}

fn save_failure(state: &DebuggerState, runner: &ContainerRunner, index: usize) -> Result<()> {
    let container_id = runner
        .container_id()
        .context("no container for failure snapshot")?
        .to_string();
    let output = state.steps[index].output.clone().unwrap_or_default();
    SessionStore::save_failure(&FailureSnapshot {
        container_id,
        step_index: index,
        step_name: state.step_name(index),
        logs: output,
        saved_at: Utc::now(),
    })
}
