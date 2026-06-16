mod snapshot;

use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::tui::DebugAction;
use crate::tui::DebuggerState;
use snapshot::from_debugger_state;

const PAGE_HTML: &str = include_str!("page.html");

#[derive(Clone)]
pub struct WebConfig {
    pub enabled: bool,
    pub port: u16,
    pub open_browser: bool,
    pub web_only: bool,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 9742,
            open_browser: true,
            web_only: false,
        }
    }
}

pub struct WebBridge {
    action_rx: mpsc::UnboundedReceiver<String>,
    snapshot_tx: broadcast::Sender<String>,
    latest: Arc<RwLock<Option<String>>>,
}

struct AppState {
    action_tx: mpsc::UnboundedSender<String>,
    snapshot_tx: broadcast::Sender<String>,
    latest: Arc<RwLock<Option<String>>>,
}

#[derive(Deserialize)]
struct ActionRequest {
    action: String,
}

pub async fn start(config: &WebConfig) -> anyhow::Result<WebBridge> {
    let (action_tx, action_rx) = mpsc::unbounded_channel();
    let (snapshot_tx, _) = broadcast::channel(128);
    let latest = Arc::new(RwLock::new(None));

    let app_state = Arc::new(AppState {
        action_tx: action_tx.clone(),
        snapshot_tx: snapshot_tx.clone(),
        latest: latest.clone(),
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/api/events", get(sse_handler))
        .route("/api/action", post(action_handler))
        .with_state(app_state);

    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let url = format!("http://{addr}");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[gh-debug] web server error: {e}");
        }
    });

    if config.open_browser {
        open_browser(&url);
    }

    eprintln!("[gh-debug] Web UI: {url}");

    Ok(WebBridge {
        action_rx,
        snapshot_tx,
        latest,
    })
}

impl WebBridge {
    pub fn publish(&self, state: &DebuggerState) {
        if let Ok(json) = serde_json::to_string(&from_debugger_state(state)) {
            if let Ok(mut guard) = self.latest.write() {
                *guard = Some(json.clone());
            }
            let _ = self.snapshot_tx.send(json);
        }
    }

    pub fn poll_action(&mut self) -> Option<DebugAction> {
        while let Ok(raw) = self.action_rx.try_recv() {
            if let Some(action) = map_web_action(&raw) {
                return Some(action);
            }
        }
        None
    }
}

fn map_web_action(raw: &str) -> Option<DebugAction> {
    match raw {
        "run" => Some(DebugAction::RunStep),
        "skip" => Some(DebugAction::SkipStep),
        "shell" => Some(DebugAction::EnterShell),
        "breakpoint" => Some(DebugAction::ToggleBreakpoint),
        "next" => Some(DebugAction::RunToBreakpoint),
        "quit" => Some(DebugAction::Quit),
        "cancel" => Some(DebugAction::Cancel),
        _ => None,
    }
}

async fn index() -> Html<&'static str> {
    Html(PAGE_HTML)
}

async fn action_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ActionRequest>,
) -> &'static str {
    let _ = state.action_tx.send(body.action);
    "ok"
}

async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let initial = state
        .latest
        .read()
        .ok()
        .and_then(|g| g.clone());

    let rx = state.snapshot_tx.subscribe();
    let updates = BroadcastStream::new(rx)
        .filter_map(|msg| msg.ok())
        .map(|json| Ok(Event::default().data(json)));

    let initial_events: Vec<Result<Event, Infallible>> = initial
        .into_iter()
        .map(|json| Ok(Event::default().data(json)))
        .collect();

    let stream = futures_util::stream::iter(initial_events).chain(updates);

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = ("open", url);
    #[cfg(target_os = "linux")]
    let cmd = ("xdg-open", url);
    #[cfg(windows)]
    let cmd = ("cmd", url);

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let _ = std::process::Command::new(cmd.0).arg(cmd.1).spawn();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new(cmd.0)
            .args(["/C", "start", cmd.1])
            .spawn();
    }
}
