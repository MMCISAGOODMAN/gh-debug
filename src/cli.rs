use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::session::{load_session, replay_session, SessionStore};
use crate::workflow::Workflow;

#[derive(Parser, Debug)]
#[command(name = "gh-debug", about = "Local GitHub Actions debugger")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run a workflow with interactive debugging
    Run {
        /// Path to workflow YAML (optional when using --resume)
        workflow: Option<PathBuf>,

        /// Job name to run
        #[arg(long)]
        job: Option<String>,

        /// Pause before step matching this name (repeatable)
        #[arg(long = "breakpoint", short = 'b')]
        breakpoints: Vec<String>,

        /// Open web dashboard in browser
        #[arg(long, default_value_t = true)]
        web: bool,

        /// Disable web dashboard
        #[arg(long)]
        no_web: bool,

        /// Web dashboard port
        #[arg(long, default_value_t = 9742)]
        web_port: u16,

        /// Web UI only (no terminal TUI)
        #[arg(long)]
        web_only: bool,

        /// Resume from last saved session
        #[arg(long)]
        resume: bool,
    },

    /// Enter shell in the last failed container
    Shell {
        #[arg(long = "last-failure")]
        last_failure: bool,
    },

    /// View logs from the last failure
    Logs {
        #[arg(long = "last-failure")]
        last_failure: bool,
    },

    /// Replay a recorded session
    Replay {
        /// Path to session JSON file
        session: PathBuf,
    },
}

impl Cli {
    pub async fn execute(self) -> Result<()> {
        match self.command {
            Commands::Run {
                workflow,
                job,
                breakpoints,
                resume,
                web,
                no_web,
                web_port,
                web_only,
            } => {
                let web_config = crate::web::WebConfig {
                    enabled: web && !no_web,
                    port: web_port,
                    open_browser: true,
                    web_only,
                };
                if resume {
                    let session = load_session()?;
                    let wf_path = session.workflow_path.clone();
                    if wf_path.as_os_str().is_empty() {
                        anyhow::bail!("resume session missing workflow path");
                    }
                    let job_name = session.job_name.clone();
                    let workflow = Workflow::from_file(&wf_path)?;
                    crate::tui::run_debugger(
                        workflow,
                        wf_path,
                        Some(job_name),
                        breakpoints,
                        Some(session),
                        web_config,
                    )
                    .await
                } else {
                    let wf_path = workflow.context("workflow path required unless --resume")?;
                    let workflow = Workflow::from_file(&wf_path)?;
                    crate::tui::run_debugger(workflow, wf_path, job, breakpoints, None, web_config)
                        .await
                }
            }
            Commands::Shell { last_failure } => {
                if !last_failure {
                    bail!("use --last-failure");
                }
                crate::docker::shell_last_failure().await
            }
            Commands::Logs { last_failure } => {
                if !last_failure {
                    bail!("use --last-failure");
                }
                let failure = SessionStore::load_failure()?;
                println!("{}", failure.logs);
                Ok(())
            }
            Commands::Replay { session } => replay_session(&session).await,
        }
    }
}
