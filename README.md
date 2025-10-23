# log-assist

A [cargo crate](https://crates.io/crates/log-assist) library **Async-friendly structured logger for [Seq](https://datalust.co/seq)** that automatically uses your Cargo project name, and safely reports panics (with file & line) even when the runtime is shutting down.

---

## ✨ Features

- ✅ **Async-friendly** — Uses Tokio background task and buffered channel  
- ✅ **Auto app name** — Uses `CARGO_PKG_NAME` automatically  
- ✅ **Safe panic hook** — Sends panic file, line, and message to Seq via blocking client (no runtime issues)  
- ✅ **Non-blocking logs** — Uses a background flush loop with configurable interval and queue size  
- ✅ **Zero config for basic use** — Just call `seq::init()` once  

---

## 🚀 Quick Start

Add to `Cargo.toml`:
```toml
[dependencies]
log-assist = "0.1.1"
anyhow = "1.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }

```

## Usage

```rs
use log_assist::{seq,TimeMode,Config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    seq::init(Config {
        application_name: "Log-Assist-Example".to_string().into(),
        endpoint: "http://localhost:5341".into(),
        api_key: None,
        queue_capacity: 10_000,
        flush_interval_ms: 1000,
        time_mode: TimeMode::Utc,
        enable_panic_hook: true
    }).await?;

    seq::info("Starting example", serde_json::json!({"env": "dev"})).await;
    seq::warn("Warning", serde_json::json!({"env": "dev"})).await;
    seq::error("Error", serde_json::json!({"env": "dev"})).await;
    panic!("Intentional panic test"); // shows panic hook behavior
}
```