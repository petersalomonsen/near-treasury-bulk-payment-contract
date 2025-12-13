//! HTTP route handlers for the Bulk Payment API
//!
//! This module defines the REST API endpoints for submitting and managing
//! payment lists.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::contract::{
    BulkPaymentClient, ListStatus, PaymentInput, PaymentList, PaymentStatus, PaymentTransaction,
};

/// Compute SHA-256 hash of payment list for verification
/// This ensures the provided list_id matches the actual payload content
fn compute_list_hash(submitter_id: &str, token_id: &str, payments: &[PaymentInput]) -> String {
    // Sort payments by recipient for deterministic ordering
    let mut sorted_payments: Vec<_> = payments.iter().collect();
    sorted_payments.sort_by(|a, b| a.recipient.cmp(&b.recipient));

    // Create canonical JSON representation
    let canonical = serde_json::json!({
        "submitter": submitter_id,
        "token_id": token_id,
        "payments": sorted_payments
    });

    // Compute SHA-256 hash
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string().as_bytes());
    let result = hasher.finalize();

    // Return hex-encoded hash
    hex::encode(result)
}

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub client: BulkPaymentClient,
    /// Track submitted lists for the worker to process
    pub pending_lists: Arc<RwLock<Vec<String>>>,
    /// RPC URL for making direct blockchain queries
    pub rpc_url: String,
}

/// Request body for submitting a payment list
#[derive(Debug, Deserialize)]
pub struct SubmitListRequest {
    pub list_id: String,
    pub submitter_id: String,
    pub dao_contract_id: String,
    pub token_id: String,
    pub payments: Vec<PaymentInput>,
}

/// Response for a submitted list
#[derive(Debug, Serialize)]
pub struct SubmitListResponse {
    pub success: bool,
    pub list_id: Option<String>,
    pub error: Option<String>,
}

/// Response for viewing a list
#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub success: bool,
    pub list: Option<PaymentListView>,
    pub error: Option<String>,
}

/// View-friendly payment list
#[derive(Debug, Serialize)]
pub struct PaymentListView {
    pub id: String,
    pub token_id: String,
    pub submitter: String,
    pub status: String,
    pub total_payments: usize,
    pub pending_payments: usize,
    pub processed_payments: usize,
    pub created_at: u64,
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub version: &'static str,
}

/// Response for payment transactions endpoint
#[derive(Debug, Serialize)]
pub struct TransactionsResponse {
    pub success: bool,
    pub transactions: Option<Vec<PaymentTransaction>>,
    pub error: Option<String>,
}

/// Response for single payment transaction hash lookup
#[derive(Debug, Serialize)]
pub struct TransactionHashResponse {
    pub success: bool,
    pub recipient: Option<String>,
    pub amount: Option<String>,
    pub block_height: Option<u64>,
    pub transaction_hash: Option<String>,
    pub error: Option<String>,
}

/// Path parameters for transaction hash lookup
#[derive(Debug, Deserialize)]
pub struct TransactionHashParams {
    pub id: String,
    pub recipient: String,
}

impl From<(String, PaymentList)> for PaymentListView {
    fn from((id, list): (String, PaymentList)) -> Self {
        let status = match list.status {
            ListStatus::Pending => "Pending",
            ListStatus::Approved => "Approved",
            ListStatus::Rejected => "Rejected",
        };

        let pending = list
            .payments
            .iter()
            .filter(|p| matches!(p.status, PaymentStatus::Pending))
            .count();
        let processed = list
            .payments
            .iter()
            .filter(|p| matches!(p.status, PaymentStatus::Paid { .. }))
            .count();

        Self {
            id,
            token_id: list.token_id,
            submitter: list.submitter,
            status: status.to_string(),
            total_payments: list.payments.len(),
            pending_payments: pending,
            processed_payments: processed,
            created_at: list.created_at,
        }
    }
}

/// Create the API router
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/submit-list", post(submit_list))
        .route("/list/:id", get(get_list))
        .route("/list/:id/transactions", get(get_transactions))
        .route(
            "/list/:id/transaction/:recipient",
            get(get_transaction_hash),
        )
        .with_state(state)
}

/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    Json(HealthResponse {
        status: "healthy",
        service: "bulk-payment-api",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Submit a new payment list
///
/// This endpoint requires a pending DAO proposal to exist with the list_id (hash)
/// as a reference. This ensures only authorized DAO members can trigger list storage.
async fn submit_list(
    State(state): State<AppState>,
    Json(request): Json<SubmitListRequest>,
) -> impl IntoResponse {
    info!(
        "Received submit-list request from {} (DAO: {}) with {} payments, list_id: {}",
        request.submitter_id,
        request.dao_contract_id,
        request.payments.len(),
        request.list_id
    );

    // First, verify the list_id matches the SHA-256 hash of the payload
    let computed_hash =
        compute_list_hash(&request.submitter_id, &request.token_id, &request.payments);
    if computed_hash != request.list_id {
        error!(
            "Hash mismatch: provided list_id {} does not match computed hash {}",
            request.list_id, computed_hash
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(SubmitListResponse {
                success: false,
                list_id: None,
                error: Some(format!(
                    "Invalid list_id: provided hash {} does not match computed hash {} of the payload. \
                     The list_id must be SHA-256(canonical_json(sorted_payments)).",
                    request.list_id, computed_hash
                )),
            }),
        );
    }

    info!("Hash verification passed for list {}", request.list_id);

    // Second, verify that a pending DAO proposal exists with this list_id
    match state
        .client
        .verify_dao_proposal(&request.dao_contract_id, &request.list_id)
        .await
    {
        Ok(true) => {
            info!(
                "DAO proposal verification passed for list {} in DAO {}",
                request.list_id, request.dao_contract_id
            );
        }
        Ok(false) => {
            error!(
                "No pending DAO proposal found for list {} in DAO {}",
                request.list_id, request.dao_contract_id
            );
            return (
                StatusCode::FORBIDDEN,
                Json(SubmitListResponse {
                    success: false,
                    list_id: None,
                    error: Some(format!(
                        "No pending DAO proposal found with list_id {} in DAO {}. \
                         Create a DAO proposal first with the list hash as reference.",
                        request.list_id, request.dao_contract_id
                    )),
                }),
            );
        }
        Err(e) => {
            error!(
                "Failed to verify DAO proposal for list {}: {}",
                request.list_id, e
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(SubmitListResponse {
                    success: false,
                    list_id: None,
                    error: Some(format!("Failed to verify DAO proposal: {}", e)),
                }),
            );
        }
    }

    // DAO proposal verified - proceed with list submission
    match state
        .client
        .submit_list(
            &request.list_id,
            &request.submitter_id,
            &request.token_id,
            request.payments,
        )
        .await
    {
        Ok(list_id) => {
            // Track this list for the worker
            {
                let mut pending = state.pending_lists.write().await;
                if !pending.contains(&list_id) {
                    pending.push(list_id.clone());
                }
            }

            (
                StatusCode::OK,
                Json(SubmitListResponse {
                    success: true,
                    list_id: Some(list_id),
                    error: None,
                }),
            )
        }
        Err(e) => {
            error!("Failed to submit payment list: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(SubmitListResponse {
                    success: false,
                    list_id: None,
                    error: Some(e.to_string()),
                }),
            )
        }
    }
}

/// Get payment list status
async fn get_list(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    info!("Received get-list request for list {}", id);

    match state.client.view_list(&id).await {
        Ok(list) => (
            StatusCode::OK,
            Json(ListResponse {
                success: true,
                list: Some((id, list).into()),
                error: None,
            }),
        ),
        Err(e) => {
            error!("Failed to get payment list {}: {}", id, e);
            (
                StatusCode::NOT_FOUND,
                Json(ListResponse {
                    success: false,
                    list: None,
                    error: Some(e.to_string()),
                }),
            )
        }
    }
}

/// Get payment transactions for a list.
/// Returns a list of recipients with their block heights where the payment was executed.
/// The block height can be used to look up the transaction on a block explorer like nearblocks.io.
async fn get_transactions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    info!("Received get-transactions request for list {}", id);

    match state.client.get_payment_transactions(&id).await {
        Ok(transactions) => (
            StatusCode::OK,
            Json(TransactionsResponse {
                success: true,
                transactions: Some(transactions),
                error: None,
            }),
        ),
        Err(e) => {
            error!("Failed to get payment transactions for list {}: {}", id, e);
            (
                StatusCode::NOT_FOUND,
                Json(TransactionsResponse {
                    success: false,
                    transactions: None,
                    error: Some(e.to_string()),
                }),
            )
        }
    }
}

/// Get the transaction hash for a single payment.
/// Looks up the block by height, finds the transaction to the bulk payment contract,
/// and returns the transaction hash.
async fn get_transaction_hash(
    State(state): State<AppState>,
    Path(params): Path<TransactionHashParams>,
) -> impl IntoResponse {
    info!(
        "Received get-transaction-hash request for list {}, recipient {}",
        params.id, params.recipient
    );

    // First, get the payment transactions to find the block_height for this recipient
    let transactions = match state.client.get_payment_transactions(&params.id).await {
        Ok(txs) => txs,
        Err(e) => {
            error!(
                "Failed to get payment transactions for list {}: {}",
                params.id, e
            );
            return (
                StatusCode::NOT_FOUND,
                Json(TransactionHashResponse {
                    success: false,
                    recipient: None,
                    amount: None,
                    block_height: None,
                    transaction_hash: None,
                    error: Some(format!("Failed to get payment transactions: {}", e)),
                }),
            );
        }
    };

    // Find the transaction for this recipient
    let payment = match transactions.iter().find(|t| t.recipient == params.recipient) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(TransactionHashResponse {
                    success: false,
                    recipient: None,
                    amount: None,
                    block_height: None,
                    transaction_hash: None,
                    error: Some(format!(
                        "Recipient {} not found in list {}",
                        params.recipient, params.id
                    )),
                }),
            );
        }
    };

    // Look up the transaction hash by querying the block
    let contract_id = state.client.get_contract_id();
    match lookup_transaction_hash(&state.rpc_url, payment.block_height, &contract_id).await {
        Ok(tx_hash) => (
            StatusCode::OK,
            Json(TransactionHashResponse {
                success: true,
                recipient: Some(payment.recipient.clone()),
                amount: Some(payment.amount.clone()),
                block_height: Some(payment.block_height),
                transaction_hash: Some(tx_hash),
                error: None,
            }),
        ),
        Err(e) => {
            error!(
                "Failed to lookup transaction hash for recipient {} in block {}: {}",
                params.recipient, payment.block_height, e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(TransactionHashResponse {
                    success: false,
                    recipient: Some(payment.recipient.clone()),
                    amount: Some(payment.amount.clone()),
                    block_height: Some(payment.block_height),
                    transaction_hash: None,
                    error: Some(format!("Failed to lookup transaction hash: {}", e)),
                }),
            )
        }
    }
}

/// Look up the transaction hash by querying the block and finding the transaction
/// to the bulk payment contract.
async fn lookup_transaction_hash(
    rpc_url: &str,
    block_height: u64,
    contract_id: &str,
) -> Result<String, anyhow::Error> {
    let client = reqwest::Client::new();

    // Query the block by height
    let block_response: serde_json::Value = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "block-query",
            "method": "block",
            "params": { "block_id": block_height }
        }))
        .send()
        .await?
        .json()
        .await?;

    if let Some(error) = block_response.get("error") {
        anyhow::bail!("RPC error getting block: {}", error);
    }

    let block = block_response
        .get("result")
        .ok_or_else(|| anyhow::anyhow!("No result in block response"))?;

    // Get chunk hashes from the block
    let chunks = block
        .get("chunks")
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow::anyhow!("No chunks in block"))?;

    // Search each chunk for transactions to the bulk payment contract
    for chunk_info in chunks {
        let chunk_hash = chunk_info
            .get("chunk_hash")
            .and_then(|h| h.as_str())
            .ok_or_else(|| anyhow::anyhow!("No chunk_hash in chunk info"))?;

        // Query the chunk
        let chunk_response: serde_json::Value = client
            .post(rpc_url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "chunk-query",
                "method": "chunk",
                "params": { "chunk_id": chunk_hash }
            }))
            .send()
            .await?
            .json()
            .await?;

        if let Some(error) = chunk_response.get("error") {
            anyhow::bail!("RPC error getting chunk: {}", error);
        }

        let chunk = chunk_response
            .get("result")
            .ok_or_else(|| anyhow::anyhow!("No result in chunk response"))?;

        // Look for transactions to the bulk payment contract
        if let Some(transactions) = chunk.get("transactions").and_then(|t| t.as_array()) {
            for tx in transactions {
                let receiver_id = tx.get("receiver_id").and_then(|r| r.as_str());
                if receiver_id == Some(contract_id) {
                    if let Some(hash) = tx.get("hash").and_then(|h| h.as_str()) {
                        return Ok(hash.to_string());
                    }
                }
            }
        }
    }

    anyhow::bail!(
        "No transaction to {} found in block {}",
        contract_id,
        block_height
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_list_hash() {
        let payments = vec![PaymentInput {
            recipient: "a.near".to_string(),
            amount: "100".to_string(),
        }];
        let hash = compute_list_hash("test.near", "native", &payments);
        println!(
            "Rust JSON: {}",
            serde_json::json!({
                "submitter": "test.near",
                "token_id": "native",
                "payments": &payments
            })
        );
        println!("Rust Hash: {}", hash);
        // serde_json alphabetizes keys: {"payments":[...],"submitter":"...","token_id":"..."}
        assert_eq!(
            hash,
            "b667f7213a94d9e4f106080e7b3ec2f92d3ad19c71c4d6cb45b2f6f370c59ec4"
        );
    }
}
