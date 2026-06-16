use anyhow::{Context, Result};
use bollard::container::{
    Config, CreateContainerOptions, LogOutput, RemoveContainerOptions, StartContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use bollard::Docker;
use futures_util::StreamExt;
use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{self, Duration, MissedTickBehavior};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    Stdout,
    Stderr,
}

pub enum ExecEvent<'a> {
    Tick,
    Line(StreamKind, &'a str),
}

pub struct ExecResult {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
    pub cancelled: bool,
}

pub fn runs_on_to_image(runs_on: &str) -> &str {
    let lower = runs_on.to_lowercase();
    match lower.as_str() {
        s if s.contains("windows") => "mcr.microsoft.com/windows/servercore:ltsc2022",
        s if s.contains("macos") => "ubuntu:22.04",
        s if s.contains("node-22") || s.contains("node22") => "node:22-bookworm",
        s if s.contains("node-20") || s.contains("node20") => "node:20-bookworm",
        s if s.contains("node-18") || s.contains("node18") => "node:18-bookworm",
        s if s.contains("python-3.12") || s.contains("python312") => "python:3.12-bookworm",
        s if s.contains("python-3.11") || s.contains("python311") => "python:3.11-bookworm",
        s if s.contains("python") => "python:3.12-bookworm",
        s if s.contains("go-1.22") || s.contains("go122") => "golang:1.22-bookworm",
        s if s.contains("go-1.21") => "golang:1.21-bookworm",
        s if s.contains("golang") || s.contains("go-") => "golang:1.22-bookworm",
        s if s.contains("java-21") => "eclipse-temurin:21-jdk-jammy",
        s if s.contains("java-17") || s.contains("java17") => "eclipse-temurin:17-jdk-jammy",
        s if s.contains("java") => "eclipse-temurin:17-jdk-jammy",
        s if s.contains("ruby-3.3") || s.contains("ruby33") => "ruby:3.3-bookworm",
        s if s.contains("ruby") => "ruby:3.3-bookworm",
        _ => "ubuntu:22.04",
    }
}

pub struct ContainerRunner {
    docker: Docker,
    container_id: Option<String>,
    workspace_host: std::path::PathBuf,
}

impl ContainerRunner {
    pub fn connect() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .context("connect to Docker (is Docker running?)")?;
        Ok(Self {
            docker,
            container_id: None,
            workspace_host: std::env::current_dir()?,
        })
    }

    pub fn with_workspace(mut self, path: std::path::PathBuf) -> Self {
        self.workspace_host = path
            .canonicalize()
            .unwrap_or(path);
        self
    }

    pub fn container_id(&self) -> Option<&str> {
        self.container_id.as_deref()
    }

    pub async fn create_job_container(
        &mut self,
        image: &str,
        env: &BTreeMap<String, String>,
    ) -> Result<()> {
        self.ensure_image(image).await?;

        let source = self.workspace_host.to_string_lossy().into_owned();
        if source.is_empty() {
            anyhow::bail!("workspace mount path is empty; run from project root or pass an absolute workflow path");
        }

        let mount = Mount {
            target: Some("/github/workspace".into()),
            source: Some(source),
            typ: Some(MountTypeEnum::BIND),
            read_only: Some(false),
            ..Default::default()
        };

        let env_list: Vec<String> = env.iter().map(|(k, v)| format!("{}={}", k, v)).collect();

        let config = Config {
            image: Some(image.into()),
            tty: Some(true),
            open_stdin: Some(true),
            env: Some(env_list),
            working_dir: Some("/github/workspace".into()),
            host_config: Some(HostConfig {
                mounts: Some(vec![mount]),
                ..Default::default()
            }),
            cmd: Some(vec!["sleep".into(), "infinity".into()]),
            ..Default::default()
        };

        let response = self
            .docker
            .create_container(None::<CreateContainerOptions<String>>, config)
            .await
            .context("create container")?;

        self.docker
            .start_container(&response.id, None::<StartContainerOptions<String>>)
            .await
            .context("start container")?;

        self.container_id = Some(response.id);
        Ok(())
    }

    async fn ensure_image(&self, image: &str) -> Result<()> {
        if self.docker.inspect_image(image).await.is_ok() {
            return Ok(());
        }
        eprintln!("[gh-debug] Pulling image {}...", image);
        let mut stream = self.docker.create_image(
            Some(bollard::image::CreateImageOptions {
                from_image: image,
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(item) = stream.next().await {
            let _ = item?;
        }
        Ok(())
    }

    pub async fn exec_command<F>(
        &self,
        command: &str,
        env: &BTreeMap<String, String>,
        mut on_event: F,
        cancel: Arc<AtomicBool>,
    ) -> Result<ExecResult>
    where
        F: FnMut(ExecEvent<'_>),
    {
        cancel.store(false, Ordering::Relaxed);

        let id = self
            .container_id
            .as_ref()
            .context("container not created")?;

        let env_list: Vec<String> = env.iter().map(|(k, v)| format!("{}={}", k, v)).collect();

        let exec = self
            .docker
            .create_exec(
                id,
                CreateExecOptions {
                    cmd: Some(vec!["bash".into(), "-lc".into(), command.into()]),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    env: Some(env_list),
                    working_dir: Some("/github/workspace".into()),
                    ..Default::default()
                },
            )
            .await?;

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut cancelled = false;

        if let StartExecResults::Attached { output: mut stream, .. } = self
            .docker
            .start_exec(&exec.id, None)
            .await?
        {
            let mut tick = time::interval(Duration::from_millis(100));
            tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }

                tokio::select! {
                    chunk = stream.next() => {
                        match chunk {
                            Some(Ok(LogOutput::StdOut { message })) => {
                                let text = String::from_utf8_lossy(&message);
                                for line in text.lines() {
                                    stdout.push_str(line);
                                    stdout.push('\n');
                                    on_event(ExecEvent::Line(StreamKind::Stdout, line));
                                }
                            }
                            Some(Ok(LogOutput::StdErr { message })) => {
                                let text = String::from_utf8_lossy(&message);
                                for line in text.lines() {
                                    stderr.push_str(line);
                                    stderr.push('\n');
                                    on_event(ExecEvent::Line(StreamKind::Stderr, line));
                                }
                            }
                            Some(Ok(_)) => {}
                            Some(Err(e)) => return Err(e.into()),
                            None => break,
                        }
                    }
                    _ = tick.tick() => {
                        on_event(ExecEvent::Tick);
                        if cancel.load(Ordering::Relaxed) {
                            cancelled = true;
                            break;
                        }
                    }
                }
            }
        }

        if cancelled {
            self.interrupt_running_command().await.ok();
            return Ok(ExecResult {
                exit_code: 130,
                stdout,
                stderr,
                cancelled: true,
            });
        }

        let inspect = self.docker.inspect_exec(&exec.id).await?;
        let exit_code = inspect.exit_code.unwrap_or(-1);
        Ok(ExecResult {
            exit_code,
            stdout,
            stderr,
            cancelled: false,
        })
    }

    /// Kill processes spawned by exec (keep container alive for debugging).
    pub async fn interrupt_running_command(&self) -> Result<()> {
        let id = self
            .container_id
            .as_ref()
            .context("container not created")?;

        let exec = self
            .docker
            .create_exec(
                id,
                CreateExecOptions::<String> {
                    cmd: Some(vec![
                        "bash".into(),
                        "-lc".into(),
                        "pkill -9 -P 1 2>/dev/null; kill -9 $(pgrep -v '^1$' 2>/dev/null) 2>/dev/null; true"
                            .into(),
                    ]),
                    attach_stdout: Some(false),
                    attach_stderr: Some(false),
                    ..Default::default()
                },
            )
            .await?;

        let _ = self.docker.start_exec(&exec.id, None).await;
        Ok(())
    }

    pub async fn interactive_shell(&self) -> Result<()> {
        let id = self
            .container_id
            .as_ref()
            .context("container not created")?;

        eprintln!("[gh-debug] Entering container shell (exit to return)...");

        let status = std::process::Command::new("docker")
            .args(["exec", "-it", id, "bash"])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("docker exec -it")?;

        if !status.success() {
            anyhow::bail!("shell exited with status {}", status);
        }
        Ok(())
    }

    pub async fn cleanup(&mut self) -> Result<()> {
        if let Some(id) = self.container_id.take() {
            let _ = self
                .docker
                .remove_container(
                    &id,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
        }
        Ok(())
    }
}

impl Drop for ContainerRunner {
    fn drop(&mut self) {
        if let Some(id) = self.container_id.take() {
            let docker = self.docker.clone();
            tokio::spawn(async move {
                let _ = docker
                    .remove_container(
                        &id,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;
            });
        }
    }
}

pub async fn shell_last_failure() -> Result<()> {
    let failure = crate::session::SessionStore::load_failure()?;
    let status = std::process::Command::new("docker")
        .args(["exec", "-it", &failure.container_id, "bash"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("docker exec into failed container")?;

    if !status.success() {
        anyhow::bail!(
            "failed to enter container {} (is it still running?)",
            failure.container_id
        );
    }
    Ok(())
}

pub fn build_run_command(run: &str, shell: Option<&str>) -> String {
    let shell = shell.unwrap_or("bash");
    let quoted = serde_json::to_string(run).unwrap_or_else(|_| format!("'{}'", run.replace('\'', "'\\''")));
    format!("{} -lc {}", shell, quoted)
}
