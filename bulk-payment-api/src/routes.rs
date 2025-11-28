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
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::contract::{BulkPaymentClient, ListStatus, PaymentInput, PaymentList, PaymentStatus};

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub client: BulkPaymentClient,
    /// Track submitted lists for the worker to process
    pub pending_lists: Arc<RwLock<Vec<u64>>>,
}

/// Request body for submitting a payment list
#[derive(Debug, Deserialize)]
pub struct SubmitListRequest {
    pub submitter_id: String,
    pub token_id: String,
    pub payments: Vec<PaymentInput>,
}

/// Response for a submitted list
#[derive(Debug, Serialize)]
pub struct SubmitListResponse {
    pub success: bool,
    pub list_id: Option<u64>,
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
    pub id: u64,
    pub token_id: String,
    pub submitter: String,
    pub status: String,
    pub total_payments: usize,
    pub pending_payments: usize,
    pub paid_payments: usize,
    pub failed_payments: usize,
    pub created_at: u64,
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
}

impl From<(u64, PaymentList)> for PaymentListView {
    fn from((id, list): (u64, PaymentList)) -> Self {
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
        let paid = list
            .payments
            .iter()
            .filter(|p| matches!(p.status, PaymentStatus::Paid))
            .count();
        let failed = list
            .payments
            .iter()
            .filter(|p| matches!(p.status, PaymentStatus::Failed { .. }))
            .count();

        Self {
            id,
            token_id: list.token_id,
            submitter: list.submitter,
            status: status.to_string(),
            total_payments: list.payments.len(),
            pending_payments: pending,
            paid_payments: paid,
            failed_payments: failed,
            created_at: list.created_at,
        }
    }
}

/// Create the API router
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/submit-list", post(submit_list))
        .route("/list/{id}", get(get_list))
        .with_state(state)
}

/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    Json(HealthResponse {
        status: "healthy".to_string(),
        service: "bulk-payment-api".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

/// Submit a new payment list
async fn submit_list(
    State(state): State<AppState>,
    Json(request): Json<SubmitListRequest>,
) -> impl IntoResponse {
    info!(
        "Received submit-list request from {} with {} payments",
        request.submitter_id,
        request.payments.len()
    );

    match state
        .client
        .submit_list(&request.submitter_id, &request.token_id, request.payments)
        .await
    {
        Ok(list_id) => {
            // Track this list for the worker
            {
                let mut pending = state.pending_lists.write().await;
                if !pending.contains(&list_id) {
                    pending.push(list_id);
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
async fn get_list(State(state): State<AppState>, Path(id): Path<u64>) -> impl IntoResponse {
    info!("Received get-list request for list {}", id);

    match state.client.view_list(id).await {
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
