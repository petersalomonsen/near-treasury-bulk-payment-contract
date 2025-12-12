//! Contract interaction logic for the Bulk Payment API
//!
//! This module handles all communication with the bulk payment smart contract
//! deployed on the NEAR sandbox.

#![allow(dead_code)]

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
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
    /// Payment was executed at the specified block height.
    /// This can be used to find the transaction on-chain.
    Paid {
        block_height: u64,
    },
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

/// Represents a completed payment transaction with block height for transaction lookup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentTransaction {
    pub recipient: String,
    pub amount: String,
    pub block_height: u64,
}

// ============================================================================
// SputnikDAO Types for Proposal Verification
// ============================================================================

/// SputnikDAO proposal status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProposalStatus {
    InProgress,
    Approved,
    Rejected,
    Removed,
    Expired,
    Moved,
    Failed,
}

/// SputnikDAO function call action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionCall {
    pub method_name: String,
    pub args: String, // Base64 encoded
    pub deposit: String,
    pub gas: String,
}

/// SputnikDAO proposal kind
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProposalKind {
    FunctionCall {
        #[serde(rename = "FunctionCall")]
        function_call: FunctionCallKind,
    },
    Other(serde_json::Value),
}

/// Function call proposal kind details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallKind {
    pub receiver_id: String,
    pub actions: Vec<ActionCall>,
}

/// SputnikDAO proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u64,
    pub proposer: String,
    pub description: String,
    pub kind: ProposalKind,
    pub status: ProposalStatus,
    pub submission_time: String,
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

    /// Get the contract ID
    pub fn get_contract_id(&self) -> &str {
        &self.contract_id
    }

    /// Verify that a pending DAO proposal exists with the given list_id (hash) as reference.
    ///
    /// This security check ensures that only authorized DAO members can trigger list storage
    /// by first creating a DAO proposal with the list hash.
    ///
    /// The method searches for pending proposals (status: InProgress) that contain:
    /// - A FunctionCall kind targeting the bulk payment contract
    /// - The list_id in the proposal description or function call args
    pub async fn verify_dao_proposal(&self, dao_contract_id: &str, list_id: &str) -> Result<bool> {
        info!(
            "Verifying DAO proposal exists for list {} in DAO {}",
            list_id, dao_contract_id
        );

        // Get the last proposal ID to know how many proposals to check
        let last_id: u64 = Contract(dao_contract_id.parse()?)
            .call_function("get_last_proposal_id", json!({}))?
            .read_only()
            .fetch_from(&self.network_config)
            .await
            .context("Failed to get last proposal ID")?
            .data;

        if last_id == 0 {
            info!("No proposals found in DAO {}", dao_contract_id);
            return Ok(false);
        }

        // Check recent proposals (last 100 or all if fewer)
        let start_id = if last_id > 100 { last_id - 100 } else { 0 };

        for proposal_id in start_id..last_id {
            match self.get_proposal(dao_contract_id, proposal_id).await {
                Ok(proposal) => {
                    // Only check InProgress proposals
                    if proposal.status != ProposalStatus::InProgress {
                        continue;
                    }

                    // Check if this proposal references our list_id
                    if self.proposal_references_list(&proposal, list_id) {
                        info!(
                            "Found matching proposal {} in DAO {} for list {}",
                            proposal_id, dao_contract_id, list_id
                        );
                        return Ok(true);
                    }
                }
                Err(e) => {
                    debug!("Failed to get proposal {}: {}", proposal_id, e);
                    continue;
                }
            }
        }

        info!(
            "No matching proposal found in DAO {} for list {}",
            dao_contract_id, list_id
        );
        Ok(false)
    }

    /// Get a specific proposal from the DAO
    async fn get_proposal(&self, dao_contract_id: &str, proposal_id: u64) -> Result<Proposal> {
        let proposal: Proposal = Contract(dao_contract_id.parse()?)
            .call_function("get_proposal", json!({ "id": proposal_id }))?
            .read_only()
            .fetch_from(&self.network_config)
            .await
            .context(format!("Failed to get proposal {}", proposal_id))?
            .data;

        Ok(proposal)
    }

    /// Check if a proposal references the given list_id
    fn proposal_references_list(&self, proposal: &Proposal, list_id: &str) -> bool {
        // Check if list_id is in the description
        if proposal.description.contains(list_id) {
            return true;
        }

        // Check if this is a FunctionCall proposal targeting our contract
        if let ProposalKind::FunctionCall { function_call } = &proposal.kind {
            // Check if targeting the bulk payment contract
            if function_call.receiver_id != self.contract_id {
                return false;
            }

            // Check each action's args for the list_id
            for action in &function_call.actions {
                // Decode base64 args and check for list_id
                if let Ok(decoded) = BASE64.decode(&action.args) {
                    if let Ok(args_str) = String::from_utf8(decoded) {
                        if args_str.contains(list_id) {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    /// Submit a new payment list to the contract
    ///
    /// The API signs as the contract account itself, and passes the submitter_id
    /// to the contract so it can record who the logical submitter is. This allows
    /// DAOs to use the API for submitting lists without needing to include the full
    /// payment list in a DAO proposal.
    pub async fn submit_list(
        &self,
        list_id: &str,
        submitter_id: &str,
        token_id: &str,
        payments: Vec<PaymentInput>,
    ) -> Result<String> {
        info!(
            "Submitting payment list {} for {} with {} payments",
            list_id,
            submitter_id,
            payments.len()
        );

        // Sign as the contract account, but pass submitter_id to the contract
        let result = Contract(self.contract_id.parse()?)
            .call_function(
                "submit_list",
                json!({
                    "list_id": list_id,
                    "token_id": token_id,
                    "payments": payments,
                    "submitter_id": submitter_id
                }),
            )?
            .transaction()
            .with_signer(self.contract_id.parse()?, self.signer.clone())
            .send_to(&self.network_config)
            .await
            .context("Failed to submit payment list")?;

        if !result.is_success() {
            anyhow::bail!("Transaction failed: {:?}", result);
        }

        // Verify the list was created by checking logs
        let log_found = result
            .logs()
            .iter()
            .any(|log| log.contains(&format!("Payment list {} submitted", list_id)));

        if !log_found {
            anyhow::bail!("List submission did not produce expected log");
        }

        info!("Payment list submitted with ID: {}", list_id);
        Ok(list_id.to_string())
    }

    /// View a payment list
    pub async fn view_list(&self, list_id: &str) -> Result<PaymentList> {
        debug!("Viewing payment list: {}", list_id);

        let result: PaymentList = Contract(self.contract_id.parse()?)
            .call_function("view_list", json!({ "list_id": list_id }))?
            .read_only()
            .fetch_from(&self.network_config)
            .await
            .context("Failed to view payment list")?
            .data;

        Ok(result)
    }

    /// Get payment transactions for a list.
    /// Returns a list of recipients with their block heights where the payment was executed.
    /// The block height can be used to look up the transaction on a block explorer.
    pub async fn get_payment_transactions(&self, list_id: &str) -> Result<Vec<PaymentTransaction>> {
        debug!("Getting payment transactions for list: {}", list_id);

        let result: Vec<PaymentTransaction> = Contract(self.contract_id.parse()?)
            .call_function("get_payment_transactions", json!({ "list_id": list_id }))?
            .read_only()
            .fetch_from(&self.network_config)
            .await
            .context("Failed to get payment transactions")?
            .data;

        Ok(result)
    }

    /// Approve a payment list
    pub async fn approve_list(&self, submitter_id: &str, list_id: &str) -> Result<()> {
        info!("Approving payment list: {}", list_id);

        // First get the list to calculate the required deposit
        let list = self.view_list(list_id).await?;

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
            .call_function("approve_list", json!({ "list_id": list_id }))?
            .transaction()
            .deposit(NearToken::from_yoctonear(total))
            .with_signer(submitter_id.parse()?, self.signer.clone())
            .send_to(&self.network_config)
            .await
            .context("Failed to approve payment list")?;

        if !result.is_success() {
            anyhow::bail!("Transaction failed: {:?}", result);
        }

        info!("Payment list {} approved", list_id);
        Ok(())
    }

    /// Execute payout batch for a payment list
    pub async fn payout_batch(
        &self,
        caller_id: &str,
        list_id: &str,
        max_payments: Option<u64>,
    ) -> Result<u64> {
        debug!(
            "Executing payout batch for list: {} with max_payments: {:?}",
            list_id, max_payments
        );

        let result = Contract(self.contract_id.parse()?)
            .call_function(
                "payout_batch",
                json!({
                    "list_id": list_id,
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

        // Parse processed count from logs (uses logs() to get logs from all receipts)
        let processed = result
            .logs()
            .iter()
            .find_map(|log| {
                if log.starts_with("Processed ") {
                    log.split_whitespace().nth(1).and_then(|s| s.parse().ok())
                } else {
                    None
                }
            })
            .unwrap_or(0);

        debug!("Processed {} payments in batch", processed);
        Ok(processed)
    }

    /// Check if a list has pending payments
    pub async fn has_pending_payments(&self, list_id: &str) -> Result<bool> {
        let list = self.view_list(list_id).await?;
        Ok(list
            .payments
            .iter()
            .any(|p| matches!(p.status, PaymentStatus::Pending)))
    }

    /// Get all approved lists that have pending payments
    pub async fn get_approved_lists_with_pending(&self) -> Result<Vec<String>> {
        // Note: In a production implementation, this would query the contract
        // for a list of approved lists. For now, we'll track them in the worker.
        warn!(
            "get_approved_lists_with_pending not fully implemented - requires contract enumeration"
        );
        Ok(vec![])
    }
}
