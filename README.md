# log-assist

**Async-friendly structured logger for [Seq](https://datalust.co/seq)** that automatically uses your Cargo project name, and safely reports panics (with file & line) even when the runtime is shutting down.

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
log-assist = "0.1"
anyhow = "1.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
