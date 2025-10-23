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