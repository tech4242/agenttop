mod config;
mod model_info;
mod otlp;
mod project;
mod providers;
mod scraper;
mod storage;
mod tui;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::providers::PROVIDER_REGISTRY;
use crate::storage::StorageHandle;

#[derive(Parser)]
#[command(name = "agenttop", about = "htop for AI coding agents", version)]
struct Args {
    /// Run in headless mode (no TUI, OTLP receiver only)
    #[arg(short = 'H', long)]
    headless: bool,

    /// Configure OTLP telemetry for a provider
    /// (claude, gemini, qwen, codex, cline, copilot, opencode, all)
    #[arg(long, value_name = "PROVIDER")]
    setup: Option<String>,
}

fn run_setup(provider_name: &str) -> Result<()> {
    let providers_to_setup: Vec<&str> = if provider_name == "all" {
        // "all" runs every provider that has auto-config; manual ones print instructions.
        vec![
            "claude", "gemini", "qwen", "copilot", "codex", "cline", "opencode",
        ]
    } else {
        vec![provider_name]
    };

    for name in providers_to_setup {
        let provider_id = match name {
            "claude" => "claude_code",
            "gemini" => "gemini_cli",
            "qwen" => "qwen_code",
            "copilot" => "copilot_chat",
            "cline" => "cline",
            "opencode" => "opencode",
            "codex" | "openai" => {
                println!("OpenAI Codex uses TOML config format (~/.codex/config.toml).");
                println!("Please configure manually. Add to your config.toml:");
                println!();
                println!("[otel]");
                println!("exporter = \"otlp-http\"");
                println!("[otel.exporter.otlp-http]");
                println!("endpoint = \"http://localhost:4318/v1/logs\"");
                println!();
                println!(
                    "Note: as of 2026-Q1, `codex exec` and `codex mcp-server` emit no telemetry"
                );
                println!("(see https://github.com/openai/codex/issues/12913).");
                println!();
                continue;
            }
            _ => {
                eprintln!("Unknown provider: {}", name);
                eprintln!(
                    "Available providers: claude, gemini, qwen, codex, cline, copilot, opencode, all"
                );
                continue;
            }
        };

        if let Some(provider) = PROVIDER_REGISTRY.get(provider_id) {
            println!("Configuring {} telemetry...", provider.name());

            match provider.ensure_configured() {
                Ok(true) => {
                    println!(
                        "  Configured {} settings at {:?}",
                        provider.name(),
                        provider.settings_path().unwrap_or_default()
                    );
                    println!(
                        "  Please restart {} for changes to take effect.",
                        provider.name()
                    );
                }
                Ok(false) => {
                    if let Some(instructions) = provider.setup_instructions() {
                        // Manual config path — print the instructions instead of "already configured".
                        println!("{}", instructions);
                    } else {
                        println!("  {} is already configured correctly.", provider.name());
                    }
                }
                Err(e) => {
                    eprintln!("  Error configuring {}: {}", provider.name(), e);
                    if let Some(instructions) = provider.setup_instructions() {
                        println!();
                        println!("{}", instructions);
                    }
                }
            }

            // For Claude Code, also install the StatusLine hook that powers
            // the in-TUI quota gauges. Best-effort — log warning on failure
            // since OTLP is the primary path.
            if provider_id == "claude_code" {
                match crate::config::install_statusline_hook() {
                    Ok(true) => {
                        println!("  Installed agenttop StatusLine hook (rate-limit capture)")
                    }
                    Ok(false) => {}
                    Err(e) => eprintln!("  Could not install StatusLine hook: {}", e),
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle --setup flag
    if let Some(provider_name) = args.setup {
        return run_setup(&provider_name);
    }

    // Initialize tracing
    // In headless mode: log to stdout
    // In TUI mode: log to file to avoid interference
    if args.headless {
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

    // Check and auto-configure Claude Code OTEL if needed (backwards compatibility)
    if let Some(claude_provider) = PROVIDER_REGISTRY.get("claude_code")
        && let Err(e) = claude_provider.ensure_configured()
    {
        eprintln!("Warning: Could not auto-configure Claude Code OTEL: {}", e);
        eprintln!("Please manually enable OTEL in ~/.claude/settings.json");
        eprintln!("Or run: agenttop --setup claude");
    }

    // Initialize storage handle (spawns storage actor thread)
    let storage = StorageHandle::new()?;

    if args.headless {
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
