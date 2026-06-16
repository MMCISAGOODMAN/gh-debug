# gh-debug

[English](#english) | [中文](#中文)

---

## English

Local GitHub Actions debugger — step through workflows, set breakpoints, enter container shells, and replay sessions **without pushing to GitHub**.

### Features

- Parse `.github/workflows/*.yml` and run steps in Docker
- **Dual UI**: terminal TUI + browser dashboard (GitHub-dark theme), synced in real time
- Breakpoints, single-step, skip, run-to-next-breakpoint
- 16+ built-in actions: `checkout`, `setup-node`, `setup-python`, `setup-go`, `cache`, artifacts, AWS/GHCR login, etc.
- Environment simulation via `.gh-debug/env.yaml`
- Failure snapshots: `shell --last-failure`, `logs --last-failure`
- Session recording and replay

### Requirements

- [Rust](https://rustup.rs/) 1.75+
- Docker (running)

### Install

```bash
git clone https://github.com/MMCISAGOODMAN/gh-debug.git
cd gh-debug
cargo install --path .
# or
cargo build --release
# binary at target/release/gh-debug
```

### Quick Start

```bash
# Interactive debug (TUI + browser, default)
gh-debug run .github/workflows/ci.yml --job lint

# Run a specific job
gh-debug run .github/workflows/ci.yml --job test
gh-debug run .github/workflows/ci.yml --job python-test

# Set breakpoint before a step (by name)
gh-debug run .github/workflows/ci.yml --breakpoint "Run tests"

# Resume from last session
gh-debug run --resume
```

### Display Modes

| Mode | Flag | Description |
|------|------|-------------|
| TUI + Web (default) | _(none)_ | Terminal debugger + browser at `http://127.0.0.1:9742` |
| Web only | `--web-only` | Browser dashboard only, no terminal TUI |
| TUI only | `--no-web` | Terminal only, no browser |
| Custom port | `--web-port 8080` | Change web dashboard port (default `9742`) |

### Web Dashboard

By default, `gh-debug run` starts a local web server and opens it in your browser. The dashboard shows step list, live logs, status, and error banner — synced with the terminal TUI via SSE.

| Button | Action |
|--------|--------|
| Run | Run current step |
| Skip | Skip current step |
| Shell | Enter container shell |
| Breakpoint | Toggle breakpoint on current step |
| Next | Run until next breakpoint |
| Cancel | Cancel running step |
| Quit | Quit and cleanup container |

When all steps complete, a green banner appears and all buttons are disabled.

### TUI Controls

| Key | Action |
|-----|--------|
| `R` | Run current step |
| `S` | Skip current step |
| `I` | Enter container shell |
| `B` | Toggle breakpoint on current step |
| `N` | Run until next breakpoint |
| `↑/↓` or `j/k` | Scroll output log |
| `PgUp/PgDn` | Scroll output by page |
| `g/G` | Jump to top/bottom of log |
| `Q` | Quit and cleanup container |
| `Q` / `Ctrl+C` / `Esc` (while running) | Cancel current step |

### Other Commands

```bash
# Enter failed container
gh-debug shell --last-failure

# View failure logs
gh-debug logs --last-failure

# Replay recorded session
gh-debug replay .gh-debug/recording.json
```

### Environment Config

Create `.gh-debug/env.yaml`:

```yaml
secrets:
  NPM_TOKEN: "test-token"
  AWS_ACCESS_KEY_ID: "local-aws-key"
  AWS_SECRET_ACCESS_KEY: "local-aws-secret"
env:
  NODE_ENV: "test"
github:
  event_name: "push"
  ref: "refs/heads/main"
  ref_name: "main"
  repository: "owner/gh-debug"
  actor: "local-dev"
```

### Example Jobs (`.github/workflows/ci.yml`)

| Job | `runs-on` | Purpose |
|-----|-----------|---------|
| `test` | node-20 | Full Node.js test suite with cache |
| `lint` | node-20 | Lint-only (quick to debug) |
| `python-test` | python-3.12 | Python setup + checks |
| `build` | ubuntu-latest | Rust toolchain + cargo build + artifact upload |
| `deploy` | ubuntu-latest | AWS/GHCR login + artifact download + github-script |

### Supported Actions

| Action | Behavior |
|--------|----------|
| `actions/checkout@v4` | Skipped (workspace mounted at `/github/workspace`) |
| `actions/setup-node@v4` | Preinstalled on `node-*` images, else binary download |
| `actions/setup-python@v5` | Preinstalled on `python-*` images, else apt |
| `actions/setup-go@v5` | Preinstalled on `golang-*` images, else official tarball |
| `actions/setup-java@v4` | Preinstalled on `java-*` images, else Temurin download |
| `actions/setup-ruby@v1` | Preinstalled on `ruby-*` images, else apt |
| `actions/setup-dotnet@v4` | dotnet-install.sh script |
| `dtolnay/rust-toolchain@stable` | rustup install (skip if rustc present) |
| `actions/cache@v4` / `cache/restore@v4` | Restore from `.gh-debug/cache/` |
| `actions/cache/save@v4` | Save to `.gh-debug/cache/` |
| `actions/upload-artifact@v4` | Save to `.gh-debug/artifacts/` |
| `actions/download-artifact@v4` | Restore from `.gh-debug/artifacts/` |
| `actions/configure-aws-credentials@v4` | Export AWS env vars (persists across steps) |
| `docker/login-action@v3` | Simulated registry login |
| `azure/login@v2` | Simulated Azure env vars |
| `actions/github-script@v7` | Run script via Node.js |

### Project Structure

```
src/
├── main.rs          # Entry point
├── cli.rs           # CLI commands
├── workflow/        # YAML parsing
├── docker/          # Container execution (bollard)
├── actions/         # Built-in action handlers
├── env/             # Environment & secrets
├── session/         # Recording, replay, failure state
├── tui/             # Interactive debugger UI (ratatui)
└── web/             # Browser dashboard (axum + SSE)
```

### License

MIT — see [LICENSE](LICENSE).

---

## 中文

本地 GitHub Actions 调试器 — 无需推送到 GitHub，即可单步执行 workflow、设置断点、进入容器 Shell、录制并重放调试会话。

### 功能特性

- 解析 `.github/workflows/*.yml`，在 Docker 中逐步执行
- **双界面**：终端 TUI + 浏览器 Dashboard（GitHub 深色主题），实时同步
- 断点、单步执行、跳过、运行至下一断点
- 内置 16+ 常用 Action：`checkout`、`setup-node`、`setup-python`、`setup-go`、`cache`、制品上传下载、AWS/GHCR 登录等
- 通过 `.gh-debug/env.yaml` 模拟 secrets 与环境变量
- 失败快照：`shell --last-failure`、`logs --last-failure`
- 会话录制与回放

### 环境要求

- [Rust](https://rustup.rs/) 1.75+
- Docker（需正在运行）

### 安装

```bash
git clone https://github.com/MMCISAGOODMAN/gh-debug.git
cd gh-debug
cargo install --path .
# 或
cargo build --release
# 可执行文件位于 target/release/gh-debug
```

### 快速开始

```bash
# 交互式调试（默认：TUI + 浏览器）
gh-debug run .github/workflows/ci.yml --job lint

# 指定 job
gh-debug run .github/workflows/ci.yml --job test
gh-debug run .github/workflows/ci.yml --job python-test

# 在指定步骤前设置断点（按步骤名称）
gh-debug run .github/workflows/ci.yml --breakpoint "Run tests"

# 从上次会话恢复
gh-debug run --resume
```

### 显示模式

| 模式 | 参数 | 说明 |
|------|------|------|
| TUI + Web（默认） | 无 | 终端调试器 + 浏览器 `http://127.0.0.1:9742` |
| 仅 Web | `--web-only` | 仅浏览器界面，不启用终端 TUI |
| 仅 TUI | `--no-web` | 仅终端界面，不打开浏览器 |
| 自定义端口 | `--web-port 8080` | 修改 Web 端口（默认 `9742`） |

### Web 控制台

默认情况下，`gh-debug run` 会启动本地 Web 服务并自动打开浏览器。页面展示步骤列表、实时日志、运行状态和错误提示，与终端 TUI 通过 SSE 实时同步。

| 按钮 | 操作 |
|------|------|
| Run | 运行当前步骤 |
| Skip | 跳过当前步骤 |
| Shell | 进入容器 Shell |
| Breakpoint | 切换当前步骤断点 |
| Next | 运行至下一断点 |
| Cancel | 取消正在运行的步骤 |
| Quit | 退出并清理容器 |

全部步骤完成后，页面会显示绿色结束提示，并禁用所有操作按钮。

### TUI 快捷键

| 按键 | 操作 |
|------|------|
| `R` | 运行当前步骤 |
| `S` | 跳过当前步骤 |
| `I` | 进入容器 Shell |
| `B` | 切换当前步骤断点 |
| `N` | 运行至下一断点 |
| `↑/↓` 或 `j/k` | 滚动日志 |
| `PgUp/PgDn` | 按页滚动日志 |
| `g/G` | 跳到日志顶部/底部 |
| `Q` | 退出并清理容器 |
| `Q` / `Ctrl+C` / `Esc`（运行中） | 取消当前步骤 |

### 其他命令

```bash
# 进入上次失败的容器
gh-debug shell --last-failure

# 查看失败日志
gh-debug logs --last-failure

# 回放录制的会话
gh-debug replay .gh-debug/recording.json
```

### 环境配置

创建 `.gh-debug/env.yaml`：

```yaml
secrets:
  NPM_TOKEN: "test-token"
  AWS_ACCESS_KEY_ID: "local-aws-key"
  AWS_SECRET_ACCESS_KEY: "local-aws-secret"
env:
  NODE_ENV: "test"
github:
  event_name: "push"
  ref: "refs/heads/main"
  ref_name: "main"
  repository: "owner/gh-debug"
  actor: "local-dev"
```

### 示例 Jobs（`.github/workflows/ci.yml`）

| Job | `runs-on` | 用途 |
|-----|-----------|------|
| `test` | node-20 | 完整 Node.js 测试（含 cache） |
| `lint` | node-20 | 仅 lint（适合快速调试） |
| `python-test` | python-3.12 | Python 环境搭建与检查 |
| `build` | ubuntu-latest | Rust 工具链 + cargo build + 制品上传 |
| `deploy` | ubuntu-latest | AWS/GHCR 登录 + 制品下载 + github-script |

### 支持的 Action

| Action | 行为 |
|--------|------|
| `actions/checkout@v4` | 跳过（工作区已挂载到 `/github/workspace`） |
| `actions/setup-node@v4` | `node-*` 镜像预装，否则下载二进制 |
| `actions/setup-python@v5` | `python-*` 镜像预装，否则 apt 安装 |
| `actions/setup-go@v5` | `golang-*` 镜像预装，否则下载官方包 |
| `actions/setup-java@v4` | `java-*` 镜像预装，否则下载 Temurin |
| `actions/setup-ruby@v1` | `ruby-*` 镜像预装，否则 apt 安装 |
| `actions/setup-dotnet@v4` | 执行 dotnet-install.sh |
| `dtolnay/rust-toolchain@stable` | rustup 安装（已有 rustc 则跳过） |
| `actions/cache@v4` / `cache/restore@v4` | 从 `.gh-debug/cache/` 恢复 |
| `actions/cache/save@v4` | 保存到 `.gh-debug/cache/` |
| `actions/upload-artifact@v4` | 保存到 `.gh-debug/artifacts/` |
| `actions/download-artifact@v4` | 从 `.gh-debug/artifacts/` 恢复 |
| `actions/configure-aws-credentials@v4` | 导出 AWS 环境变量（跨步骤持久化） |
| `docker/login-action@v3` | 模拟镜像仓库登录 |
| `azure/login@v2` | 模拟 Azure 环境变量 |
| `actions/github-script@v7` | 通过 Node.js 执行脚本 |

### 项目结构

```
src/
├── main.rs          # 入口
├── cli.rs           # CLI 命令
├── workflow/        # YAML 解析
├── docker/          # 容器执行（bollard）
├── actions/         # 内置 Action 处理器
├── env/             # 环境变量与 secrets
├── session/         # 录制、回放、失败状态
├── tui/             # 终端交互界面（ratatui）
└── web/             # 浏览器 Dashboard（axum + SSE）
```

### 许可证

MIT — 详见 [LICENSE](LICENSE)。
