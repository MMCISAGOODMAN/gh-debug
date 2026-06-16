use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EnvConfig {
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub github: GitHubContext,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GitHubContext {
    #[serde(default)]
    pub event_name: Option<String>,
    #[serde(default)]
    pub ref_name: Option<String>,
    #[serde(default, rename = "ref")]
    pub git_ref: Option<String>,
    #[serde(default)]
    pub sha: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub workflow: Option<String>,
    #[serde(default)]
    pub job: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub run_number: Option<String>,
}

impl EnvConfig {
    pub fn load_from_project(project_root: &Path) -> Result<Self> {
        let path = project_root.join(".gh-debug/env.yaml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("read env config {}", path.display()))?;
        let config: EnvConfig =
            serde_yaml::from_str(&content).context("parse .gh-debug/env.yaml")?;
        Ok(config)
    }

    pub fn github_env_vars(&self, workflow_name: &str, job_name: &str) -> BTreeMap<String, String> {
        let mut vars = BTreeMap::new();
        let gh = &self.github;

        vars.insert(
            "GITHUB_EVENT_NAME".into(),
            gh.event_name.clone().unwrap_or_else(|| "push".into()),
        );
        vars.insert(
            "GITHUB_REF".into(),
            gh.git_ref
                .clone()
                .unwrap_or_else(|| "refs/heads/main".into()),
        );
        vars.insert(
            "GITHUB_REF_NAME".into(),
            gh.ref_name.clone().unwrap_or_else(|| "main".into()),
        );
        vars.insert(
            "GITHUB_SHA".into(),
            gh.sha
                .clone()
                .unwrap_or_else(|| "0000000000000000000000000000000000000000".into()),
        );
        vars.insert(
            "GITHUB_REPOSITORY".into(),
            gh.repository
                .clone()
                .unwrap_or_else(|| "owner/repo".into()),
        );
        vars.insert(
            "GITHUB_ACTOR".into(),
            gh.actor.clone().unwrap_or_else(|| "local-dev".into()),
        );
        vars.insert(
            "GITHUB_WORKFLOW".into(),
            gh.workflow.clone().unwrap_or_else(|| workflow_name.into()),
        );
        vars.insert("GITHUB_JOB".into(), gh.job.clone().unwrap_or_else(|| job_name.into()));
        vars.insert(
            "GITHUB_RUN_ID".into(),
            gh.run_id.clone().unwrap_or_else(|| "1".into()),
        );
        vars.insert(
            "GITHUB_RUN_NUMBER".into(),
            gh.run_number.clone().unwrap_or_else(|| "1".into()),
        );
        vars.insert("GITHUB_WORKSPACE".into(), "/github/workspace".into());
        vars.insert("CI".into(), "true".into());
        vars.insert("RUNNER_OS".into(), "Linux".into());
        vars.insert("RUNNER_ARCH".into(), "X64".into());
        vars.insert("RUNNER_TEMP".into(), "/tmp".into());
        vars.insert("RUNNER_TOOL_CACHE".into(), "/opt/hostedtoolcache".into());

        vars
    }

    pub fn merged_env(&self, job_env: Option<&BTreeMap<String, serde_yaml::Value>>) -> BTreeMap<String, String> {
        let mut env = self.github_env_vars("", "");
        for (k, v) in &self.env {
            env.insert(k.clone(), v.clone());
        }
        if let Some(job) = job_env {
            for (k, v) in job {
                if let Some(s) = value_to_string(v) {
                    env.insert(k.clone(), s);
                }
            }
        }
        env
    }
}

pub fn value_to_string(v: &serde_yaml::Value) -> Option<String> {
    match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        serde_yaml::Value::Null => None,
        other => Some(format!("{:?}", other)),
    }
}

pub fn step_env(
    base: &BTreeMap<String, String>,
    step_env: Option<&BTreeMap<String, serde_yaml::Value>>,
) -> BTreeMap<String, String> {
    let mut env = base.clone();
    if let Some(step) = step_env {
        for (k, v) in step {
            if let Some(s) = value_to_string(v) {
                env.insert(k.clone(), s);
            }
        }
    }
    env
}

pub fn inject_secrets(env: &mut BTreeMap<String, String>, secrets: &BTreeMap<String, String>) {
    for (k, v) in secrets {
        env.insert(format!("SECRETS_{}", k), v.clone());
        env.entry(k.clone()).or_insert_with(|| v.clone());
    }
}
