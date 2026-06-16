use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Workflow {
    pub name: Option<String>,
    pub jobs: BTreeMap<String, Job>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    #[serde(rename = "runs-on")]
    pub runs_on: RunsOn,
    pub env: Option<BTreeMap<String, serde_yaml::Value>>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RunsOn {
    Single(String),
    Matrix(Vec<serde_yaml::Value>),
}

impl RunsOn {
    pub fn primary(&self) -> String {
        match self {
            RunsOn::Single(s) => s.clone(),
            RunsOn::Matrix(v) => v
                .first()
                .and_then(|x| x.as_str())
                .unwrap_or("ubuntu-latest")
                .to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub name: Option<String>,
    pub uses: Option<String>,
    #[serde(rename = "with")]
    pub with: Option<BTreeMap<String, serde_yaml::Value>>,
    pub run: Option<String>,
    pub env: Option<BTreeMap<String, serde_yaml::Value>>,
    #[serde(default)]
    pub shell: Option<String>,
}

impl Step {
    pub fn display_name(&self, index: usize) -> String {
        if let Some(name) = &self.name {
            name.clone()
        } else if let Some(uses) = &self.uses {
            uses.clone()
        } else if let Some(run) = &self.run {
            let first_line = run.lines().next().unwrap_or("run");
            if first_line.len() > 40 {
                format!("{}...", &first_line[..40])
            } else {
                first_line.to_string()
            }
        } else {
            format!("Step {}", index + 1)
        }
    }
}

impl Workflow {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("read workflow {}", path.display()))?;
        let workflow: Workflow = serde_yaml::from_str(&content)
            .with_context(|| format!("parse workflow {}", path.display()))?;
        Ok(workflow)
    }

    pub fn job_names(&self) -> Vec<String> {
        self.jobs.keys().cloned().collect()
    }

    pub fn get_job(&self, name: &str) -> Result<&Job> {
        self.jobs
            .get(name)
            .ok_or_else(|| anyhow!("job '{}' not found; available: {:?}", name, self.job_names()))
    }

    pub fn resolve_job(&self, name: Option<String>) -> Result<(String, &Job)> {
        if let Some(name) = name {
            let job = self.get_job(&name)?;
            Ok((name, job))
        } else if self.jobs.len() == 1 {
            let (name, job) = self.jobs.iter().next().unwrap();
            Ok((name.clone(), job))
        } else {
            bail!(
                "multiple jobs found ({:?}), specify --job",
                self.job_names()
            )
        }
    }
}

pub fn default_workflow_dir(workflow_path: &Path) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    let abs = if workflow_path.is_absolute() {
        workflow_path.to_path_buf()
    } else {
        cwd.join(workflow_path)
    };

    let root = abs
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(|p| {
            if p.as_os_str().is_empty() {
                cwd.clone()
            } else {
                p.to_path_buf()
            }
        })
        .unwrap_or_else(|| cwd.clone());

    root.canonicalize().unwrap_or(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_root_from_relative_workflow_path() {
        let cwd = std::env::current_dir().expect("cwd");
        let root = default_workflow_dir(Path::new(".github/workflows/ci.yml"));
        assert!(!root.as_os_str().is_empty());
        assert_eq!(root, cwd.canonicalize().unwrap_or(cwd));
    }
}
