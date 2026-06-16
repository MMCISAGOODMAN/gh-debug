use anyhow::{anyhow, Result};
use std::collections::BTreeMap;

pub fn resolve_action(uses: &str, with: Option<&BTreeMap<String, serde_yaml::Value>>) -> Result<ActionResult> {
    let normalized = uses.trim().to_lowercase();
    let with = with.cloned().unwrap_or_default();

    match normalized.as_str() {
        // ── Source ──────────────────────────────────────────────
        a if a.starts_with("actions/checkout@") => checkout(),

        // ── Language setup ────────────────────────────────────
        a if a.starts_with("actions/setup-node@") => setup_node(&with),
        a if a.starts_with("actions/setup-python@") => setup_python(&with),
        a if a.starts_with("actions/setup-go@") => setup_go(&with),
        a if a.starts_with("actions/setup-java@") => setup_java(&with),
        a if a.starts_with("actions/setup-ruby@") => setup_ruby(&with),
        a if a.starts_with("actions/setup-dotnet@") => setup_dotnet(&with),
        a if a.starts_with("dtolnay/rust-toolchain@") || a.starts_with("actions-rs/toolchain@") => {
            setup_rust(&with)
        }

        // ── Cache ───────────────────────────────────────────────
        a if a.starts_with("actions/cache@") => cache_restore(&with),
        a if a.starts_with("actions/cache/restore@") => cache_restore(&with),
        a if a.starts_with("actions/cache/save@") => cache_save(&with),

        // ── Artifacts ───────────────────────────────────────────
        a if a.starts_with("actions/upload-artifact@") => upload_artifact(&with),
        a if a.starts_with("actions/download-artifact@") => download_artifact(&with),

        // ── Cloud / registry ────────────────────────────────────
        a if a.starts_with("actions/configure-aws-credentials@") => configure_aws(&with),
        a if a.starts_with("docker/login-action@") => docker_login(&with),
        a if a.starts_with("azure/login@") => azure_login(&with),

        // ── Utility ─────────────────────────────────────────────
        a if a.starts_with("actions/github-script@") => github_script(&with),
        a if a.starts_with("actions/upload-pages-artifact@") => upload_artifact(&with),

        other => Err(anyhow!(
            "unsupported action '{}'\n\
             Supported: checkout, setup-node/python/go/java/ruby/dotnet/rust-toolchain, \
             cache, upload/download-artifact, configure-aws-credentials, docker/login, \
             azure/login, github-script",
            other
        )),
    }
}

pub struct ActionResult {
    pub description: String,
    pub commands: Vec<String>,
    pub skip_container_exec: bool,
    pub env_exports: BTreeMap<String, String>,
}

impl ActionResult {
    fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            commands: vec![],
            skip_container_exec: false,
            env_exports: BTreeMap::new(),
        }
    }
}

fn checkout() -> Result<ActionResult> {
    Ok(ActionResult {
        skip_container_exec: true,
        ..ActionResult::new("Code mounted via volume (no fetch)")
    })
}

fn setup_node(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let version = get_with_str(with, "node-version").unwrap_or_else(|| "20".into());
    let major = version.split('.').next().unwrap_or("20");
    let full = node_release(major);
    Ok(ActionResult {
        commands: vec![shell(format!(
            "if command -v node >/dev/null 2>&1; then \
               echo 'Node already available:'; node --version && npm --version; \
             else \
               echo 'Downloading Node.js v{full}...'; \
               apt-get update -qq && apt-get install -y -qq curl ca-certificates xz-utils; \
               curl -fsSL https://nodejs.org/dist/v{full}/node-v{full}-linux-x64.tar.xz \
                 | tar -xJ -C /usr/local --strip-components=1; \
               node --version && npm --version; \
             fi"
        ))],
        ..ActionResult::new(format!("Node.js {version} (preinstalled or binary download)"))
    })
}

fn setup_python(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let version = get_with_str(with, "python-version").unwrap_or_else(|| "3.12".into());
    Ok(ActionResult {
        commands: vec![shell(format!(
            "if command -v python3 >/dev/null 2>&1; then \
               echo 'Python already available:'; python3 --version; \
             else \
               apt-get update -qq && apt-get install -y -qq python3 python3-pip python3-venv; \
               python3 --version; \
             fi"
        ))],
        ..ActionResult::new(format!("Python {version}"))
    })
}

fn setup_go(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let version = get_with_str(with, "go-version").unwrap_or_else(|| "1.22".into());
    let full = go_release(&version);
    Ok(ActionResult {
        commands: vec![shell(format!(
            "if command -v go >/dev/null 2>&1; then \
               echo 'Go already available:'; go version; \
             else \
               apt-get update -qq && apt-get install -y -qq curl ca-certificates; \
               curl -fsSL https://go.dev/dl/go{full}.linux-amd64.tar.gz | tar -C /usr/local -xzf -; \
               export PATH=/usr/local/go/bin:$PATH; \
               go version; \
             fi"
        ))],
        env_exports: btmap(&[("PATH", "/usr/local/go/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")]),
        ..ActionResult::new(format!("Go {version}"))
    })
}

fn setup_java(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let version = get_with_str(with, "java-version").unwrap_or_else(|| "17".into());
    let major = version.split('.').next().unwrap_or("17");
    Ok(ActionResult {
        commands: vec![shell(format!(
            "if command -v java >/dev/null 2>&1; then \
               echo 'Java already available:'; java -version; \
             else \
               apt-get update -qq && apt-get install -y -qq curl ca-certificates; \
               curl -fsSL 'https://api.adoptium.net/v3/binary/latest/{major}/ga/linux/x64/jdk/hotspot/normal/eclipse' \
                 -o /tmp/jdk.tar.gz; \
               mkdir -p /opt/java && tar -xzf /tmp/jdk.tar.gz -C /opt/java --strip-components=1; \
               /opt/java/bin/java -version; \
             fi"
        ))],
        env_exports: btmap(&[
            ("JAVA_HOME", "/opt/java"),
            ("PATH", "/opt/java/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"),
        ]),
        ..ActionResult::new(format!("Java {version} (Temurin)"))
    })
}

fn setup_ruby(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let version = get_with_str(with, "ruby-version").unwrap_or_else(|| "3.3".into());
    Ok(ActionResult {
        commands: vec![shell(
            "if command -v ruby >/dev/null 2>&1; then \
               echo 'Ruby already available:'; ruby --version; \
             else \
               apt-get update -qq && apt-get install -y -qq ruby-full; \
               ruby --version; \
             fi",
        )],
        ..ActionResult::new(format!("Ruby {version}"))
    })
}

fn setup_dotnet(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let version = get_with_str(with, "dotnet-version").unwrap_or_else(|| "8.0".into());
    Ok(ActionResult {
        commands: vec![shell(format!(
            "if command -v dotnet >/dev/null 2>&1; then \
               echo '.NET already available:'; dotnet --version; \
             else \
               apt-get update -qq && apt-get install -y -qq curl; \
               curl -fsSL https://dot.net/v1/dotnet-install.sh | bash -s -- --channel {version}; \
               export PATH=$HOME/.dotnet:$PATH; \
               $HOME/.dotnet/dotnet --version; \
             fi"
        ))],
        env_exports: btmap(&[("PATH", "/root/.dotnet:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")]),
        ..ActionResult::new(format!(".NET {version}"))
    })
}

fn setup_rust(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let toolchain = get_with_str(with, "toolchain")
        .or_else(|| get_with_str(with, "version"))
        .unwrap_or_else(|| "stable".into());
    Ok(ActionResult {
        commands: vec![shell(format!(
            "if command -v rustc >/dev/null 2>&1; then \
               echo 'Rust already available:'; rustc --version && cargo --version; \
             else \
               curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
                 | sh -s -- -y --default-toolchain {toolchain}; \
               source $HOME/.cargo/env; \
               rustc --version && cargo --version; \
             fi"
        ))],
        env_exports: btmap(&[("PATH", "/root/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")]),
        ..ActionResult::new(format!("Rust toolchain {toolchain}"))
    })
}

fn cache_restore(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let path = get_with_str(with, "path").unwrap_or_else(|| ".cache".into());
    let key = get_with_str(with, "key").unwrap_or_else(|| "default-key".into());
    Ok(ActionResult {
        commands: vec![shell(format!(
            "mkdir -p /github/workspace/.gh-debug/cache/{key} /github/workspace/{path}; \
             if [ -d /github/workspace/.gh-debug/cache/{key}/{path} ]; then \
               echo 'Cache hit: {key}'; \
               cp -a /github/workspace/.gh-debug/cache/{key}/{path}/. /github/workspace/{path}/ 2>/dev/null || true; \
             else \
               echo 'Cache miss: {key}'; \
             fi"
        ))],
        ..ActionResult::new(format!("Restore cache {key} → {path}"))
    })
}

fn cache_save(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let path = get_with_str(with, "path").unwrap_or_else(|| ".cache".into());
    let key = get_with_str(with, "key").unwrap_or_else(|| "default-key".into());
    Ok(ActionResult {
        commands: vec![shell(format!(
            "mkdir -p /github/workspace/.gh-debug/cache/{key}/{path}; \
             if [ -d /github/workspace/{path} ]; then \
               cp -a /github/workspace/{path}/. /github/workspace/.gh-debug/cache/{key}/{path}/ 2>/dev/null || true; \
               echo 'Cache saved: {key}'; \
             fi"
        ))],
        ..ActionResult::new(format!("Save cache {path} → {key}"))
    })
}

fn upload_artifact(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let name = get_with_str(with, "name").unwrap_or_else(|| "artifact".into());
    let path = get_with_str(with, "path").unwrap_or_else(|| ".".into());
    Ok(ActionResult {
        commands: vec![shell(format!(
            "dest=/github/workspace/.gh-debug/artifacts/{name}; \
             mkdir -p \"$dest\"; \
             if [ -e /github/workspace/{path} ]; then \
               cp -a /github/workspace/{path} \"$dest/\"; \
               echo \"Uploaded artifact '{name}' from {path}\"; \
               ls -la \"$dest\"; \
             else \
               echo \"Warning: path {path} not found, creating empty artifact\"; \
               touch \"$dest/.keep\"; \
             fi"
        ))],
        ..ActionResult::new(format!("Upload artifact '{name}'"))
    })
}

fn download_artifact(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let name = get_with_str(with, "name").unwrap_or_else(|| "artifact".into());
    let path = get_with_str(with, "path").unwrap_or_else(|| ".".into());
    Ok(ActionResult {
        commands: vec![shell(format!(
            "src=/github/workspace/.gh-debug/artifacts/{name}; \
             mkdir -p /github/workspace/{path}; \
             if [ -d \"$src\" ]; then \
               cp -a \"$src/\"* /github/workspace/{path}/ 2>/dev/null || true; \
               echo \"Downloaded artifact '{name}' to {path}\"; \
             else \
               echo \"Warning: artifact '{name}' not found locally\"; \
             fi"
        ))],
        ..ActionResult::new(format!("Download artifact '{name}'"))
    })
}

fn configure_aws(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let key_id = get_with_str(with, "aws-access-key-id").unwrap_or_else(|| "local-dev-key".into());
    let secret = get_with_str(with, "aws-secret-access-key").unwrap_or_else(|| "local-dev-secret".into());
    let region = get_with_str(with, "aws-region").unwrap_or_else(|| "us-east-1".into());
    let mut result = ActionResult::new(format!("Configure AWS credentials (region: {region})"));
    result.skip_container_exec = true;
    result.env_exports = btmap(&[
        ("AWS_ACCESS_KEY_ID", key_id.as_str()),
        ("AWS_SECRET_ACCESS_KEY", secret.as_str()),
        ("AWS_DEFAULT_REGION", region.as_str()),
        ("AWS_REGION", region.as_str()),
    ]);
    Ok(result)
}

fn docker_login(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let registry = get_with_str(with, "registry").unwrap_or_else(|| "ghcr.io".into());
    Ok(ActionResult {
        skip_container_exec: true,
        ..ActionResult::new(format!("Docker login to {registry} (simulated)"))
    })
}

fn azure_login(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let mut result = ActionResult::new("Azure login (simulated with env vars)");
    result.skip_container_exec = true;
    result.env_exports = btmap(&[
        ("AZURE_CLIENT_ID", "local-dev-client-id"),
        ("AZURE_TENANT_ID", "local-dev-tenant-id"),
        ("AZURE_SUBSCRIPTION_ID", "local-dev-subscription-id"),
    ]);
    if let Some(creds) = get_with_str(with, "creds") {
        result.env_exports.insert("AZURE_CREDENTIALS".into(), creds);
    }
    Ok(result)
}

fn github_script(with: &BTreeMap<String, serde_yaml::Value>) -> Result<ActionResult> {
    let script = get_with_str(with, "script").unwrap_or_else(|| "console.log('github-script ok')".into());
    let escaped = script.replace('\\', "\\\\").replace('\'', "'\\''");
    Ok(ActionResult {
        commands: vec![shell(format!(
            "if ! command -v node >/dev/null 2>&1; then \
               echo 'github-script requires Node.js — run setup-node first'; exit 1; \
             fi; \
             node -e '{escaped}'"
        ))],
        ..ActionResult::new("Run GitHub script via Node.js")
    })
}

fn shell(body: impl Into<String>) -> String {
    format!("bash -lc '{}'", body.into().replace('\'', "'\\''"))
}

fn get_with_str(with: &BTreeMap<String, serde_yaml::Value>, key: &str) -> Option<String> {
    with.get(key).and_then(|v| match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    })
}

fn node_release(major: &str) -> &'static str {
    match major {
        "22" => "22.12.0",
        "20" => "20.18.1",
        "18" => "18.20.5",
        _ => "20.18.1",
    }
}

fn go_release(version: &str) -> String {
    if version.contains('.') && version.matches('.').count() >= 2 {
        version.to_string()
    } else {
        format!("{version}.0")
    }
}

fn btmap(pairs: &[(&str, impl AsRef<str>)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.as_ref().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_common_actions() {
        assert!(resolve_action("actions/checkout@v4", None).is_ok());
        assert!(resolve_action("actions/setup-node@v4", None).is_ok());
        assert!(resolve_action("actions/upload-artifact@v4", None).is_ok());
        assert!(resolve_action("docker/login-action@v3", None).is_ok());
        assert!(resolve_action("dtolnay/rust-toolchain@stable", None).is_ok());
    }
}
