mod config;
mod otlp;
mod storage;
mod tui;

use anyhow::Result;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::storage::StorageHandle;

#[tokio::main]
async fn main() -> Result<()> {
    let headless = std::env::args().any(|arg| arg == "--headless" || arg == "-H");

    // Initialize tracing
    // In headless mode: log to stdout
    // In TUI mode: log to file to avoid interference
    if headless {
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(
                std::env::var("RUST_LOG").unwrap_or_else(|_| "agenttop=info".into()),
            ))
            .with(fmt::layer())
            .init();
    } else {
        let log_dir = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("agenttop");
        std::fs::create_dir_all(&log_dir)?;
        let log_file = std::fs::File::create(log_dir.join("agenttop.log"))?;

        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::new(
                std::env::var("RUST_LOG").unwrap_or_else(|_| "agenttop=info".into()),
            ))
            .with(fmt::layer().with_writer(log_file).with_ansi(false))
            .init();
    }

    // Check and auto-configure Claude Code OTEL if needed
    if let Err(e) = config::ensure_otel_configured() {
        eprintln!("Warning: Could not auto-configure Claude Code OTEL: {}", e);
        eprintln!("Please manually enable OTEL in ~/.claude/settings.json");
    }

    // Initialize storage handle (spawns storage actor thread)
    let storage = StorageHandle::new()?;

    if headless {
        // Headless mode: just run the OTLP receiver
        tracing::info!("Running in headless mode (no TUI)");
        tracing::info!("OTLP endpoint: http://127.0.0.1:4318");
        tracing::info!("Press Ctrl+C to stop");

        otlp::start_receiver(storage).await?;
    } else {
        // Start OTLP receiver in background
        let otlp_storage = storage.clone();
        tokio::spawn(async move {
            if let Err(e) = otlp::start_receiver(otlp_storage).await {
                tracing::error!("OTLP receiver error: {}", e);
            }
        });

        // Run TUI (this blocks until quit)
        tui::run(storage).await?;
    }

    Ok(())
}
