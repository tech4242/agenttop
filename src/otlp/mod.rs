use anyhow::Result;
use axum::{Router, body::Bytes, extract::State, http::StatusCode, routing::post};
use tower_http::cors::CorsLayer;

use crate::storage::StorageHandle;

pub mod parser;

pub use parser::*;

pub async fn start_receiver(storage: StorageHandle) -> Result<()> {
    let app = Router::new()
        .route("/v1/metrics", post(handle_metrics))
        .route("/v1/logs", post(handle_logs))
        .route("/v1/traces", post(handle_traces))
        .layer(CorsLayer::permissive())
        .with_state(storage);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:4318").await?;
    tracing::info!("OTLP receiver listening on http://127.0.0.1:4318");

    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_metrics(State(storage): State<StorageHandle>, body: Bytes) -> StatusCode {
    tracing::debug!("Received metrics: {} bytes", body.len());

    match parser::parse_metrics(&body) {
        Ok(metrics) => {
            for metric in metrics {
                match metric {
                    ParsedMetric::TokenUsage { token_type, count } => {
                        storage.record_token_usage(&token_type, count);
                    }
                    ParsedMetric::CostUsage { cost_usd } => {
                        storage.record_cost(cost_usd);
                    }
                    ParsedMetric::SessionMetric { name, value } => {
                        storage.record_session_metric(&name, value);
                    }
                }
            }
            StatusCode::OK
        }
        Err(e) => {
            tracing::error!("Failed to parse metrics: {}", e);
            StatusCode::BAD_REQUEST
        }
    }
}

async fn handle_logs(State(storage): State<StorageHandle>, body: Bytes) -> StatusCode {
    tracing::debug!("Received logs: {} bytes", body.len());

    match parser::parse_logs(&body) {
        Ok(events) => {
            tracing::debug!("Parsed {} log events", events.len());
            for event in &events {
                tracing::debug!(
                    "  event.name={:?}, attributes={:?}",
                    event.event_name,
                    event.attributes.keys().collect::<Vec<_>>()
                );
            }
            // Store all log events without filtering - filtering happens at query time
            storage.record_log_events(events);
            StatusCode::OK
        }
        Err(e) => {
            tracing::error!("Failed to parse logs: {}", e);
            StatusCode::BAD_REQUEST
        }
    }
}

async fn handle_traces(State(_storage): State<StorageHandle>, body: Bytes) -> StatusCode {
    // Traces are not used currently, but we accept them
    tracing::debug!("Received traces: {} bytes", body.len());
    StatusCode::OK
}
