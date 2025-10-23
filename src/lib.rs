//! log-assist: async-friendly Seq logger that auto-uses your Cargo project name.
//!
//! Usage:
//! ```rust
//! use log_assist::seq;
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Usually read these from your own config file once:
//!     seq::init(seq::Config {
//!         endpoint: "http://localhost:5341".into(), // Seq URL (no trailing slash)
//!         api_key: None,                             // Some("YOUR_SEQ_API_KEY".into())
//!         queue_capacity: 10_000,                    // optional tuning
//!         flush_interval_ms: 1000,                   // batch flush cadence
//!     }).await?;
//!
//!     seq::info("service started", serde_json::json!({"port": 8080})).await;
//!     Ok(())
//! }
//! ```

use once_cell::sync::OnceCell;
use reqwest::Client;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc::{self, Sender};
use tokio::time::{self, Duration};

const APP_NAME: &str = env!("CARGO_PKG_VERSION"); // <-- pulled from your Cargo project name

static STATE: OnceCell<Arc<SeqState>> = OnceCell::new();

#[derive(Debug, Clone, Copy)]
pub enum TimeMode {
    Utc,
    Local,
}

impl Default for TimeMode {
    fn default() -> Self {
        TimeMode::Utc
    }
}
#[derive(Debug, Clone)]
pub struct Config {
    pub application_name: Option<String>, // optional override of app name
    pub endpoint: String,        // e.g., http://seq.yourdomain:5341
    pub api_key: Option<String>, // X-Seq-ApiKey
    pub queue_capacity: usize,   // channel size
    pub flush_interval_ms: u64,  // batching cadence
    pub time_mode: TimeMode,     // NEW (Utc by default)
    pub enable_panic_hook: bool, // default true
}

#[derive(Clone)]
struct SeqState {
    tx: Sender<Event>,
    time_mode: TimeMode, // NEW
    app_name: String,
}

#[derive(Debug, Error)]
pub enum InitError {
    #[error("already initialized")]
    AlreadyInitialized,
    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Debug, Error)]
pub enum LogError {
    #[error("logger not initialized")]
    NotInitialized,
    #[error("queue full, message dropped")]
    QueueFull,
}

#[derive(Debug, Serialize)]
struct Event {
    #[serde(rename = "@t")]
    timestamp: String, // RFC3339
    #[serde(rename = "@mt")]
    message_template: String,
    #[serde(rename = "@l")]
    level: String,
    #[serde(rename = "Application")]
    application: String,
    #[serde(flatten)]
    properties: serde_json::Value,
}

fn now_rfc3339(mode: TimeMode) -> String {
    use ::time::format_description::well_known::Rfc3339;

    match mode {
        TimeMode::Utc => ::time::OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
        TimeMode::Local => {
            let dt = ::time::OffsetDateTime::now_local()
                .unwrap_or_else(|_| ::time::OffsetDateTime::now_utc());
            dt.format(&Rfc3339)
                .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
        }
    }
}

/// Public module façade
pub mod seq {

    use super::*;

    /// Initialize once (spawn background shipper task).
    pub async fn init(cfg: Config) -> Result<(), InitError> {
        if STATE.get().is_some() {
            return Err(InitError::AlreadyInitialized);
        }
        if cfg.enable_panic_hook {
            // pass url and api-key
            install_panic_hook(cfg.clone());
        }

        let (tx, mut rx) = mpsc::channel::<Event>(cfg.queue_capacity);
        let state = Arc::new(SeqState {
            tx,
            time_mode: cfg.time_mode,
            app_name: cfg
                .application_name
                .clone()
                .unwrap_or_else(|| APP_NAME.to_string()),
        });
        let endpoint = cfg.endpoint.trim_end_matches('/').to_string();
        let api_key = cfg.api_key.clone();
        let client = Client::builder().build()?;
        let flush_every = Duration::from_millis(cfg.flush_interval_ms);

        // background shipper
        tokio::spawn(async move {
            let mut buf: Vec<Event> = Vec::with_capacity(1024);
            let mut ticker = time::interval(flush_every);

            loop {
                tokio::select! {
                    maybe_ev = rx.recv() => {
                        match maybe_ev {
                            Some(ev) => { buf.push(ev); }
                            None => { // channel closed => drain and exit
                                let _ = try_flush(&client, &endpoint, api_key.as_deref(), &mut buf).await;
                                break;
                            }
                        }
                    }
                    _ = ticker.tick() => {
                        let _ = try_flush(&client, &endpoint, api_key.as_deref(), &mut buf).await;
                    }
                }
            }
        });

        STATE.set(state).ok();
        Ok(())
    }

    /// Info log (non-blocking enqueue; send happens in background).
    pub async fn info(message: &str, props: serde_json::Value) {
        let _ = enqueue("Information", message, props).await;
    }

    /// Warning log.
    pub async fn warn(message: &str, props: serde_json::Value) {
        let _ = enqueue("Warning", message, props).await;
    }

    /// Error log.
    pub async fn error(message: &str, props: serde_json::Value) {
        let _ = enqueue("Error", message, props).await;
    }

    /// Convenience when you don’t care about extra properties.
    pub async fn info_msg(message: &str) {
        info(message, json!({})).await
    }
    pub async fn warn_msg(message: &str) {
        warn(message, json!({})).await
    }
    pub async fn error_msg(message: &str) {
        error(message, json!({})).await
    }

    async fn enqueue(level: &str, message: &str, props: serde_json::Value) -> Result<(), LogError> {
        let state = STATE.get().ok_or(LogError::NotInitialized)?.clone();
        // let now = OffsetDateTime::now_utc().format(&::time::format_description::well_known::Rfc3339).unwrap();
        let now = now_rfc3339(state.time_mode);

        let mut properties = props;
        // Always include app name; user props can override if they *really* want.
        if !properties.get("App").is_some() {
            if let Some(obj) = properties.as_object_mut() {
                obj.insert("App".into(), json!(state.app_name));
            }
        }

        let ev = Event {
            timestamp: now,
            message_template: message.to_string(),
            level: level.to_string(),
            application: state.app_name.to_string(),
            properties,
        };

        state.tx.try_send(ev).map_err(|_| LogError::QueueFull)?;
        Ok(())
    }

    async fn try_flush(
        client: &Client,
        endpoint: &str,
        api_key: Option<&str>,
        buf: &mut Vec<Event>,
    ) -> Result<(), reqwest::Error> {
        if buf.is_empty() {
            return Ok(());
        }

        // Convert to CLEF lines (each event one line)
        let clef_lines = buf
            .iter()
            .map(|e| serde_json::to_string(e).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n");

        let url = format!("{}/api/events/raw?clef", endpoint);
        let mut req = client
            .post(url)
            .body(clef_lines)
            .header("Content-Type", "application/vnd.serilog.clef");

        if let Some(key) = api_key {
            req = req.header("X-Seq-ApiKey", key);
        }

        let res = req.send().await?;
        if res.status().is_success() {
            buf.clear(); // only clear on success
        } else {
            eprintln!(
                "SEQ response {}: {}",
                res.status(),
                res.text().await.unwrap_or_default()
            );
        }

        Ok(())
    }
}
use std::panic;

/// Installs a global panic hook that logs to Seq and stderr.
/// This will be automatically called by `seq::init()` after initialization.
// ---------- Panic hook (blocking send; safe during runtime teardown) ----------
fn install_panic_hook(config: Config) {
    // Clone only the fields needed for the panic hook and move them into the closure
    #[derive(Clone)]
    struct PanicConfig {
        endpoint: String,
        api_key: Option<String>,
    }
    let panic_cfg = PanicConfig {
        endpoint: config.endpoint.clone(),
        api_key: config.api_key.clone(),
    };

    panic::set_hook(Box::new(move |panic_info| {
        let location = panic_info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown".into());

        let msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".into()
        };

        eprintln!("💥 Panic at {location}: {msg}");

        if let Some(state) = STATE.get() {
            // Build a single CLEF line for the panic event
            let ev = Event {
                timestamp: now_rfc3339(state.time_mode),
                message_template: "Panic occurred".to_string(),
                level: "Error".to_string(),
                application: state.app_name.to_string(),
                properties: serde_json::json!({
                    "location": location,
                    "message": msg,
                    "thread": std::thread::current().name(),
                }),
            };

            send_event_sync(&panic_cfg.endpoint, panic_cfg.api_key.as_deref(), &ev);
        }
    }));
}

fn send_event_sync(endpoint: &str, api_key: Option<&str>, event: &Event) {
    if let Ok(line) = serde_json::to_string(event) {
        if let Ok(client) = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(800))
            .build()
        {
            let mut req = client
                .post(format!("{}/api/events/raw?clef", endpoint))
                .header("Content-Type", "application/vnd.serilog.clef")
                .body(line);
            if let Some(key) = api_key {
                req = req.header("X-Seq-ApiKey", key);
            }
            let _ = req.send();
        }
    }
}
