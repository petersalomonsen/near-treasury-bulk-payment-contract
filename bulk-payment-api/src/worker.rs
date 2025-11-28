//! Background worker for processing approved payment lists
//!
//! This worker polls the contract for approved lists and executes
//! payout batches until all payments are complete.

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::contract::{BulkPaymentClient, ListStatus};

/// Worker configuration
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// How often to poll for approved lists (in seconds)
    pub poll_interval: u64,
    /// Maximum payments to process per batch
    pub max_payments_per_batch: u64,
    /// Caller account ID for executing payouts
    pub caller_id: String,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            poll_interval: 5,
            max_payments_per_batch: 100,
            caller_id: "test.near".to_string(),
        }
    }
}

/// Background worker for processing payouts
pub struct PayoutWorker {
    client: BulkPaymentClient,
    config: WorkerConfig,
    /// Track lists that need processing
    pending_lists: Arc<RwLock<Vec<u64>>>,
}

impl PayoutWorker {
    /// Create a new payout worker
    ///
    /// Note: This function cannot be const because `BulkPaymentClient` contains
    /// types that don't support const construction (e.g., `Arc<Signer>`).
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(
        client: BulkPaymentClient,
        config: WorkerConfig,
        pending_lists: Arc<RwLock<Vec<u64>>>,
    ) -> Self {
        Self {
            client,
            config,
            pending_lists,
        }
    }

    /// Start the worker loop
    pub async fn run(&self) -> Result<()> {
        info!(
            "Starting payout worker with {}s poll interval",
            self.config.poll_interval
        );

        let mut poll_interval = interval(Duration::from_secs(self.config.poll_interval));

        loop {
            poll_interval.tick().await;
            if let Err(e) = self.process_pending_lists().await {
                error!("Error processing pending lists: {}", e);
            }
        }
    }

    /// Process all pending lists
    async fn process_pending_lists(&self) -> Result<()> {
        // Get a copy of the pending lists
        let lists: Vec<u64> = {
            let pending = self.pending_lists.read().await;
            pending.clone()
        };

        if lists.is_empty() {
            debug!("No pending lists to process");
            return Ok(());
        }

        debug!("Processing {} pending lists", lists.len());

        let mut lists_to_remove = Vec::new();

        for list_id in lists {
            match self.process_list(list_id).await {
                Ok(complete) => {
                    if complete {
                        lists_to_remove.push(list_id);
                    }
                }
                Err(e) => {
                    warn!("Error processing list {}: {}", list_id, e);
                }
            }
        }

        // Remove completed lists
        if !lists_to_remove.is_empty() {
            self.pending_lists
                .write()
                .await
                .retain(|id| !lists_to_remove.contains(id));
            info!(
                "Removed {} completed lists from worker",
                lists_to_remove.len()
            );
        }

        Ok(())
    }

    /// Process a single payment list
    ///
    /// Returns true if the list is complete (no more pending payments)
    async fn process_list(&self, list_id: u64) -> Result<bool> {
        // Get the list status
        let list = self.client.view_list(list_id).await?;

        match list.status {
            ListStatus::Pending => {
                debug!("List {} is still pending approval", list_id);
                return Ok(false);
            }
            ListStatus::Rejected => {
                info!("List {} was rejected, removing from queue", list_id);
                return Ok(true);
            }
            ListStatus::Approved => {
                // Continue to process
            }
        }

        // Check if there are pending payments
        let pending_count = list
            .payments
            .iter()
            .filter(|p| matches!(p.status, crate::contract::PaymentStatus::Pending))
            .count();

        if pending_count == 0 {
            info!("List {} has no pending payments, complete!", list_id);
            return Ok(true);
        }

        info!(
            "Processing list {} with {} pending payments",
            list_id, pending_count
        );

        // Execute payout batch
        let processed = self
            .client
            .payout_batch(
                &self.config.caller_id,
                list_id,
                Some(self.config.max_payments_per_batch),
            )
            .await?;

        info!(
            "Processed {} payments for list {}, {} remaining",
            processed,
            list_id,
            pending_count.saturating_sub(processed as usize)
        );

        // Check if complete
        let has_pending = self.client.has_pending_payments(list_id).await?;
        Ok(!has_pending)
    }
}
