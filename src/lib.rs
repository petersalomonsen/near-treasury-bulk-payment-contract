// NEAR Treasury Bulk Payment Contract
// See: https://github.com/NEAR-DevHub/near-treasury/issues/101
use near_sdk::{near, env, AccountId, NearToken, Promise, require, log, Gas};
use near_sdk::store::UnorderedMap;
use near_sdk::json_types::U128;

#[near(contract_state)]
pub struct BulkPaymentContract {
    payment_lists: UnorderedMap<u64, PaymentList>,
    storage_credits: UnorderedMap<AccountId, NearToken>,
    next_list_id: u64,
    approval_deposits: UnorderedMap<u64, NearToken>,
}

#[near(serializers = [json])]
pub struct PaymentInput {
    pub recipient: AccountId,
    pub amount: U128,
}

#[near(serializers = [json, borsh])]
#[derive(Clone)]
pub struct PaymentRecord {
    pub recipient: AccountId,
    pub amount: u128,
    pub status: PaymentStatus,
}

#[near(serializers = [json, borsh])]
#[derive(Clone)]
pub enum PaymentStatus {
    Pending,
    Paid,
    Failed { error: String },
}

#[near(serializers = [json, borsh])]
#[derive(Clone)]
pub struct PaymentList {
    pub token_id: String,
    pub submitter: AccountId,
    pub status: ListStatus,
    pub payments: Vec<PaymentRecord>,
    pub created_at: u64,
}

#[near(serializers = [json, borsh])]
#[derive(Clone)]
pub enum ListStatus {
    Pending,
    Approved,
    Rejected,
}

impl Default for BulkPaymentContract {
    fn default() -> Self {
        Self {
            payment_lists: UnorderedMap::new(b"p"),
            storage_credits: UnorderedMap::new(b"s"),
            next_list_id: 0,
            approval_deposits: UnorderedMap::new(b"d"),
        }
    }
}

#[near]
impl BulkPaymentContract {
    /// Calculate storage cost for payment records with 10% markup
    /// Storage includes: AccountId (max 100 chars) + u128 amount + status fields
    #[payable]
    pub fn buy_storage(&mut self, num_records: u64) -> NearToken {
        require!(num_records > 0, "Number of records must be greater than 0");
        
        // Calculate storage per record:
        // - AccountId: 100 bytes max (UTF-8 string)
        // - amount: 16 bytes (u128)
        // - status: ~50 bytes (enum with error string)
        // - overhead: ~50 bytes for Vec storage
        const BYTES_PER_RECORD: u64 = 216;
        
        let storage_bytes = BYTES_PER_RECORD * num_records;
        
        // NEAR storage cost: 1 byte = 10^19 yoctoNEAR
        let storage_cost_yocto = storage_bytes as u128 * 10_u128.pow(19);
        
        // Add 10% revenue margin
        let total_cost_yocto = storage_cost_yocto * 11 / 10;
        let total_cost = NearToken::from_yoctonear(total_cost_yocto);
        
        let attached = env::attached_deposit();
        require!(
            attached == total_cost,
            format!("Exact deposit required: {}, attached: {}", total_cost, attached)
        );
        
        // Track storage credits per account
        let caller = env::predecessor_account_id();
        let current_credits = self.storage_credits.get(&caller).copied().unwrap_or(NearToken::from_yoctonear(0));
        let new_credits = NearToken::from_yoctonear(
            current_credits.as_yoctonear() + (num_records as u128)
        );
        self.storage_credits.insert(caller.clone(), new_credits);
        
        log!("Storage purchased: {} records for {}", num_records, total_cost);
        
        total_cost
    }
    
    /// Submit a payment list with pending status
    pub fn submit_list(&mut self, token_id: String, payments: Vec<PaymentInput>) -> u64 {
        require!(!payments.is_empty(), "Payment list cannot be empty");
        
        let caller = env::predecessor_account_id();
        
        // Verify storage credits
        let required_credits = payments.len() as u128;
        let current_credits = self.storage_credits.get(&caller).copied()
            .unwrap_or(NearToken::from_yoctonear(0))
            .as_yoctonear();
        
        require!(
            current_credits >= required_credits,
            format!("Insufficient storage credits. Required: {}, Available: {}", required_credits, current_credits)
        );
        
        // Deduct storage credits
        let new_credits = NearToken::from_yoctonear(current_credits - required_credits);
        self.storage_credits.insert(caller.clone(), new_credits);
        
        // Convert PaymentInput to PaymentRecord with Pending status
        let payment_records: Vec<PaymentRecord> = payments
            .into_iter()
            .map(|input| PaymentRecord {
                recipient: input.recipient,
                amount: input.amount.0,
                status: PaymentStatus::Pending,
            })
            .collect();
        
        // Create payment list
        let list_id = self.next_list_id;
        self.next_list_id += 1;
        
        let payment_list = PaymentList {
            token_id,
            submitter: caller.clone(),
            status: ListStatus::Pending,
            payments: payment_records,
            created_at: env::block_timestamp(),
        };
        
        self.payment_lists.insert(list_id, payment_list);
        
        log!("Payment list {} submitted by {} with {} payments", list_id, caller, self.payment_lists.get(&list_id).unwrap().payments.len());
        
        list_id
    }
    
    /// Approve a payment list and attach the exact deposit amount
    #[payable]
    pub fn approve_list(&mut self, list_ref: u64) {
        let caller = env::predecessor_account_id();
        
        let mut list = self.payment_lists.get(&list_ref)
            .expect("Payment list not found")
            .clone();
        
        require!(
            list.submitter == caller,
            "Only the submitter can approve the list"
        );
        
        require!(
            matches!(list.status, ListStatus::Pending),
            "List must be in Pending status"
        );
        
        // Calculate total payment amount
        let total_amount: u128 = list.payments.iter()
            .map(|p| p.amount)
            .sum();
        
        let attached = env::attached_deposit();
        let required = NearToken::from_yoctonear(total_amount);
        
        require!(
            attached == required,
            format!("Exact deposit required: {}, attached: {}", required, attached)
        );
        
        // Update list status
        list.status = ListStatus::Approved;
        self.payment_lists.insert(list_ref, list);
        
        // Store approval deposit for potential refund
        self.approval_deposits.insert(list_ref, attached);
        
        log!("Payment list {} approved with deposit {}", list_ref, attached);
    }
    
    /// Process payments in batches (public function, anyone can call)
    pub fn payout_batch(&mut self, list_ref: u64, max_payments: Option<u64>) {
        let max = max_payments.unwrap_or(100).min(100);
        
        let mut list = self.payment_lists.get(&list_ref)
            .expect("Payment list not found")
            .clone();
        
        require!(
            matches!(list.status, ListStatus::Approved),
            "List must be Approved to process payments"
        );
        
        let mut processed = 0;
        for payment in list.payments.iter_mut() {
            if processed >= max {
                break;
            }
            
            if matches!(payment.status, PaymentStatus::Pending) {
                // Process payment based on token type
                if list.token_id.starts_with("nep141:") {
                    // NEAR Intents - call ft_withdraw on intents.near
                    let token = list.token_id.strip_prefix("nep141:").unwrap();
                    
                    // Call ft_withdraw on intents.near
                    let args = format!(
                        r#"{{"token":"{}","receiver_id":"{}","amount":"{}"}}"#,
                        token,
                        payment.recipient,
                        payment.amount
                    );
                    
                    Promise::new("intents.near".parse().unwrap())
                        .function_call(
                            "ft_withdraw".to_string(),
                            args.into_bytes(),
                            NearToken::from_yoctonear(1),
                            Gas::from_tgas(50),
                        );
                    
                    // Mark as Paid (in real implementation, would use callbacks)
                    payment.status = PaymentStatus::Paid;
                } else if list.token_id == "native" {
                    // Native NEAR transfer
                    Promise::new(payment.recipient.clone())
                        .transfer(NearToken::from_yoctonear(payment.amount));
                    
                    payment.status = PaymentStatus::Paid;
                } else {
                    payment.status = PaymentStatus::Failed {
                        error: "Unsupported token type".to_string(),
                    };
                }
                
                processed += 1;
            }
        }
        
        // Update the list
        self.payment_lists.insert(list_ref, list);
        
        log!("Processed {} payments for list {}", processed, list_ref);
    }
    
    /// Reject a payment list and refund any approval deposit
    pub fn reject_list(&mut self, list_ref: u64) {
        let caller = env::predecessor_account_id();
        
        let mut list = self.payment_lists.get(&list_ref)
            .expect("Payment list not found")
            .clone();
        
        require!(
            list.submitter == caller,
            "Only the submitter can reject the list"
        );
        
        require!(
            !matches!(list.status, ListStatus::Rejected),
            "List is already rejected"
        );
        
        // Update status
        list.status = ListStatus::Rejected;
        self.payment_lists.insert(list_ref, list);
        
        // Refund approval deposit if any
        if let Some(deposit) = self.approval_deposits.get(&list_ref).copied() {
            Promise::new(caller.clone()).transfer(deposit);
            self.approval_deposits.remove(&list_ref);
            log!("Payment list {} rejected, refunded {}", list_ref, deposit);
        } else {
            log!("Payment list {} rejected", list_ref);
        }
    }
    
    /// View a payment list with all details
    pub fn view_list(&self, list_ref: u64) -> PaymentList {
        self.payment_lists.get(&list_ref)
            .expect("Payment list not found")
            .clone()
    }
    
    /// Reset failed payments to pending for approved lists
    pub fn retry_failed(&mut self, list_ref: u64) {
        let caller = env::predecessor_account_id();
        
        let mut list = self.payment_lists.get(&list_ref)
            .expect("Payment list not found")
            .clone();
        
        require!(
            list.submitter == caller,
            "Only the submitter can retry failed payments"
        );
        
        require!(
            matches!(list.status, ListStatus::Approved),
            "List must be Approved to retry failed payments"
        );
        
        let mut retry_count = 0;
        for payment in list.payments.iter_mut() {
            if matches!(payment.status, PaymentStatus::Failed { .. }) {
                payment.status = PaymentStatus::Pending;
                retry_count += 1;
            }
        }
        
        self.payment_lists.insert(list_ref, list);
        
        log!("Reset {} failed payments to pending for list {}", retry_count, list_ref);
    }
    
    /// View storage credits for an account
    pub fn view_storage_credits(&self, account_id: AccountId) -> NearToken {
        self.storage_credits.get(&account_id)
            .copied()
            .unwrap_or(NearToken::from_yoctonear(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialization() {
        let contract = BulkPaymentContract::default();
        assert_eq!(contract.next_list_id, 0);
    }
}
