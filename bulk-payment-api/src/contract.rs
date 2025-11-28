//! Contract interaction logic for the Bulk Payment API
//!
//! This module handles all communication with the bulk payment smart contract
//! deployed on the NEAR sandbox.

#![allow(dead_code)]

use anyhow::{Context, Result};
use near_api::{Contract, NearGas, NearToken, NetworkConfig, RPCEndpoint, Signer};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Payment input for submitting to the contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentInput {
    pub recipient: String,
    pub amount: String,
}

/// Payment record returned from the contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRecord {
    pub recipient: String,
    pub amount: String,
    pub status: PaymentStatus,
}

/// Payment status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PaymentStatus {
    Pending,
    Paid,
    Failed { error: String },
}

/// List status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ListStatus {
    Pending,
    Approved,
    Rejected,
}

/// Payment list returned from the contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentList {
    pub token_id: String,
    pub submitter: String,
    pub status: ListStatus,
    pub payments: Vec<PaymentRecord>,
    pub created_at: u64,
}

/// Client for interacting with the bulk payment contract
#[derive(Clone)]
pub struct BulkPaymentClient {
    network_config: NetworkConfig,
    contract_id: String,
    signer: Arc<Signer>,
}

impl BulkPaymentClient {
    /// Create a new bulk payment client
    pub fn new(rpc_url: &str, contract_id: &str, signer: Arc<Signer>) -> Self {
        let network_config = NetworkConfig {
            network_name: "sandbox".to_string(),
            rpc_endpoints: vec![RPCEndpoint::new(rpc_url.parse().unwrap())],
            linkdrop_account_id: None,
            ..NetworkConfig::testnet()
        };

        Self {
            network_config,
            contract_id: contract_id.to_string(),
            signer,
        }
    }

    /// Create a client with the genesis signer (for testing in sandbox mode)
    ///
    /// # Security Note
    /// This uses the well-known NEAR sandbox genesis private key which is public and
    /// intended only for local testing. It is the same key used by near-sandbox.
    /// See: https://github.com/near/sandbox
    ///
    /// For production use, load credentials from environment variables or secure storage.
    pub fn with_genesis_signer(rpc_url: &str, contract_id: &str) -> Result<Self> {
        // The near-sandbox genesis private key is a well-known test key, not a secret.
        // It is publicly documented and hardcoded in near-sandbox for testing purposes.
        // See: near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY
        let genesis_private_key = std::env::var("NEAR_SIGNER_PRIVATE_KEY").unwrap_or_else(|_| {
            "ed25519:3D4YudUQRE39Lc4JHghuB5WM8kbgDDa34mnrEP5DdTApVH81af7e2dWgNPEaiQfdJnZq1CNPp5im4Rg5b733oiMP".to_string()
        });

        let signer = Signer::new(Signer::from_secret_key(genesis_private_key.parse()?))?;

        Ok(Self::new(rpc_url, contract_id, signer))
    }

    /// Submit a new payment list to the contract
    pub async fn submit_list(
        &self,
        submitter_id: &str,
        token_id: &str,
        payments: Vec<PaymentInput>,
    ) -> Result<u64> {
        info!(
            "Submitting payment list for {} with {} payments",
            submitter_id,
            payments.len()
        );

        let result = Contract(self.contract_id.parse()?)
            .call_function(
                "submit_list",
                json!({
                    "token_id": token_id,
                    "payments": payments
                }),
            )?
            .transaction()
            .with_signer(submitter_id.parse()?, self.signer.clone())
            .send_to(&self.network_config)
            .await
            .context("Failed to submit payment list")?;

        if !result.is_success() {
            anyhow::bail!("Transaction failed: {:?}", result);
        }

        // Parse the list ID from the return value
        let list_id: u64 = result
            .outcome()
            .logs
            .iter()
            .find_map(|log| {
                if log.starts_with("Payment list ") {
                    log.split_whitespace()
                        .nth(2)
                        .and_then(|s| s.parse().ok())
                } else {
                    None
                }
            })
            .context("Failed to parse list ID from logs")?;

        info!("Payment list submitted with ID: {}", list_id);
        Ok(list_id)
    }

    /// View a payment list
    pub async fn view_list(&self, list_ref: u64) -> Result<PaymentList> {
        debug!("Viewing payment list: {}", list_ref);

        let result: PaymentList = Contract(self.contract_id.parse()?)
            .call_function("view_list", json!({ "list_ref": list_ref }))?
            .read_only()
            .fetch_from(&self.network_config)
            .await
            .context("Failed to view payment list")?
            .data;

        Ok(result)
    }

    /// Approve a payment list
    pub async fn approve_list(&self, submitter_id: &str, list_ref: u64) -> Result<()> {
        info!("Approving payment list: {}", list_ref);

        // First get the list to calculate the required deposit
        let list = self.view_list(list_ref).await?;

        // Calculate total payment amount, returning error for invalid amounts
        let total: u128 = list
            .payments
            .iter()
            .map(|p| {
                p.amount
                    .parse::<u128>()
                    .map_err(|e| anyhow::anyhow!("Invalid payment amount '{}': {}", p.amount, e))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum();

        let result = Contract(self.contract_id.parse()?)
            .call_function("approve_list", json!({ "list_ref": list_ref }))?
            .transaction()
            .deposit(NearToken::from_yoctonear(total))
            .with_signer(submitter_id.parse()?, self.signer.clone())
            .send_to(&self.network_config)
            .await
            .context("Failed to approve payment list")?;

        if !result.is_success() {
            anyhow::bail!("Transaction failed: {:?}", result);
        }

        info!("Payment list {} approved", list_ref);
        Ok(())
    }

    /// Execute payout batch for a payment list
    pub async fn payout_batch(
        &self,
        caller_id: &str,
        list_ref: u64,
        max_payments: Option<u64>,
    ) -> Result<u64> {
        debug!(
            "Executing payout batch for list: {} with max_payments: {:?}",
            list_ref, max_payments
        );

        let result = Contract(self.contract_id.parse()?)
            .call_function(
                "payout_batch",
                json!({
                    "list_ref": list_ref,
                    "max_payments": max_payments
                }),
            )?
            .transaction()
            .gas(NearGas::from_tgas(300))
            .with_signer(caller_id.parse()?, self.signer.clone())
            .send_to(&self.network_config)
            .await
            .context("Failed to execute payout batch")?;

        if !result.is_success() {
            anyhow::bail!("Payout batch transaction failed: {:?}", result);
        }

        // Parse processed count from logs
        let processed = result
            .outcome()
            .logs
            .iter()
            .find_map(|log| {
                if log.starts_with("Processed ") {
                    log.split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse().ok())
                } else {
                    None
                }
            })
            .unwrap_or(0);

        debug!("Processed {} payments in batch", processed);
        Ok(processed)
    }

    /// Check if a list has pending payments
    pub async fn has_pending_payments(&self, list_ref: u64) -> Result<bool> {
        let list = self.view_list(list_ref).await?;
        Ok(list
            .payments
            .iter()
            .any(|p| matches!(p.status, PaymentStatus::Pending)))
    }

    /// Get all approved lists that have pending payments
    pub async fn get_approved_lists_with_pending(&self) -> Result<Vec<u64>> {
        // Note: In a production implementation, this would query the contract
        // for a list of approved lists. For now, we'll track them in the worker.
        warn!("get_approved_lists_with_pending not fully implemented - requires contract enumeration");
        Ok(vec![])
    }
}
