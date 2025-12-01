//! Bulk Payment API - Main Entry Point
//!
//! This is the REST API server for the NEAR Treasury Bulk Payment system.
//! It provides endpoints for submitting payment lists and checking their status,
//! along with a background worker that processes approved lists.

mod contract;
mod routes;
mod worker;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use contract::BulkPaymentClient;
use routes::{create_router, AppState};
use worker::{PayoutWorker, WorkerConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting Bulk Payment API v{}", env!("CARGO_PKG_VERSION"));

    // Read configuration from environment
    let rpc_url = std::env::var("NEAR_RPC_URL").unwrap_or_else(|_| "http://localhost:3030".into());
    let contract_id = std::env::var("BULK_PAYMENT_CONTRACT_ID")
        .unwrap_or_else(|_| "bulk-payment.test.near".into());
    let api_port = std::env::var("API_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080u16);
    let worker_caller = std::env::var("WORKER_CALLER_ID").unwrap_or_else(|_| "test.near".into());

    info!("Configuration:");
    info!("  RPC URL: {}", rpc_url);
    info!("  Contract ID: {}", contract_id);
    info!("  API Port: {}", api_port);
    info!("  Worker Caller: {}", worker_caller);

    // Create the bulk payment client
    let client = BulkPaymentClient::with_genesis_signer(&rpc_url, &contract_id)?;

    // Shared state for tracking pending lists
    let pending_lists = Arc::new(RwLock::new(Vec::new()));

    // Create app state
    let app_state = AppState {
        client: client.clone(),
        pending_lists: pending_lists.clone(),
    };

    // Create the router with CORS and tracing
    let app = create_router(app_state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http());

    // Start the background worker
    let worker_config = WorkerConfig {
        poll_interval: 5,
        max_payments_per_batch: 10, // Keep small to avoid gas issues
        caller_id: worker_caller,
    };
    let worker = PayoutWorker::new(client, worker_config, pending_lists);

    tokio::spawn(async move {
        if let Err(e) = worker.run().await {
            tracing::error!("Worker error: {}", e);
        }
    });

    // Start the HTTP server
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", api_port)).await?;
    info!("API server listening on 0.0.0.0:{}", api_port);

    axum::serve(listener, app).await?;

    Ok(())
}
