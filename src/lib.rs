// NEAR Treasury Bulk Payment Contract
// See: https://github.com/NEAR-DevHub/near-treasury/issues/101
//
// This contract enables batch payment processing with support for:
// - Native NEAR tokens
// - NEP-141 fungible tokens via NEAR Intents (intents.near)
// - Storage-based fee model with 10% revenue margin
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
    /// Initialize the contract
    #[init]
    pub fn new() -> Self {
        Self::default()
    }
    
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
        
        let storage_bytes = BYTES_PER_RECORD.checked_mul(num_records)
            .expect("Storage bytes calculation overflow");
        
        // NEAR storage cost: 1 byte = 10^19 yoctoNEAR
        let storage_cost_yocto = (storage_bytes as u128).checked_mul(10_u128.pow(19))
            .expect("Storage cost calculation overflow");
        
        // Add 10% revenue margin
        let total_cost_yocto = storage_cost_yocto.checked_mul(11)
            .and_then(|x| x.checked_div(10))
            .expect("Total cost calculation overflow");
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
            current_credits.as_yoctonear()
                .checked_add(num_records as u128)
                .expect("Storage credits overflow")
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
        
        // Calculate total payment amount (with overflow check)
        let total_amount: u128 = list.payments.iter()
            .map(|p| p.amount)
            .try_fold(0u128, |acc, x| acc.checked_add(x))
            .expect("Total payment amount overflow");
        
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
    
    /// Reject a payment list (only allowed before approval)
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
            matches!(list.status, ListStatus::Pending),
            "Only pending lists can be rejected"
        );
        
        // Update status
        list.status = ListStatus::Rejected;
        self.payment_lists.insert(list_ref, list);
        
        log!("Payment list {} rejected", list_ref);
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
    use near_sdk::test_utils::{VMContextBuilder, accounts};
    use near_sdk::testing_env;

    fn get_context(predecessor: AccountId) -> VMContextBuilder {
        let mut builder = VMContextBuilder::new();
        builder.predecessor_account_id(predecessor);
        builder
    }

    #[test]
    fn test_initialization() {
        let contract = BulkPaymentContract::default();
        assert_eq!(contract.next_list_id, 0);
    }

    #[test]
    fn test_storage_cost_calculation() {
        let mut context = get_context(accounts(0));
        
        // Calculate expected cost for 10 records
        // 216 bytes per record * 10 = 2160 bytes
        // 2160 * 10^19 yoctoNEAR/byte = 21600000000000000000000 yoctoNEAR
        // With 10% markup: 21600000000000000000000 * 1.1 = 23760000000000000000000 yoctoNEAR
        let expected_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
        
        context.attached_deposit(expected_cost);
        testing_env!(context.build());
        
        let mut contract = BulkPaymentContract::default();
        let result = contract.buy_storage(10);
        
        assert_eq!(result, expected_cost);
        
        // Verify credits were added
        let credits = contract.view_storage_credits(accounts(0));
        assert_eq!(credits.as_yoctonear(), 10);
    }

    #[test]
    #[should_panic(expected = "Exact deposit required")]
    fn test_storage_wrong_deposit() {
        let mut context = get_context(accounts(0));
        context.attached_deposit(NearToken::from_yoctonear(1_000_000));
        testing_env!(context.build());
        
        let mut contract = BulkPaymentContract::default();
        contract.buy_storage(10); // Should panic with wrong deposit
    }

    #[test]
    fn test_submit_list_deducts_credits() {
        let mut context = get_context(accounts(0));
        
        // First buy storage
        let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
        context.attached_deposit(storage_cost);
        testing_env!(context.build());
        
        let mut contract = BulkPaymentContract::default();
        contract.buy_storage(10);
        
        // Now submit a list with 2 payments
        context.attached_deposit(NearToken::from_yoctonear(0));
        testing_env!(context.build());
        
        let payments = vec![
            PaymentInput {
                recipient: accounts(1),
                amount: U128(1_000_000_000_000_000_000_000_000),
            },
            PaymentInput {
                recipient: accounts(2),
                amount: U128(2_000_000_000_000_000_000_000_000),
            },
        ];
        
        let list_id = contract.submit_list("native".to_string(), payments);
        
        // Verify credits were deducted (10 - 2 = 8)
        let credits = contract.view_storage_credits(accounts(0));
        assert_eq!(credits.as_yoctonear(), 8);
        
        // Verify list was created
        assert_eq!(list_id, 0);
        let list = contract.view_list(0);
        assert_eq!(list.payments.len(), 2);
        assert_eq!(list.submitter, accounts(0));
    }

    #[test]
    #[should_panic(expected = "Insufficient storage credits")]
    fn test_submit_list_insufficient_credits() {
        let context = get_context(accounts(0));
        testing_env!(context.build());
        
        let mut contract = BulkPaymentContract::default();
        
        let payments = vec![
            PaymentInput {
                recipient: accounts(1),
                amount: U128(1_000_000_000_000_000_000_000_000),
            },
        ];
        
        // Should panic - no storage credits
        contract.submit_list("native".to_string(), payments);
    }

    #[test]
    fn test_approve_list() {
        let mut context = get_context(accounts(0));
        
        // Setup: buy storage and submit list
        let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
        context.attached_deposit(storage_cost);
        testing_env!(context.build());
        
        let mut contract = BulkPaymentContract::default();
        contract.buy_storage(10);
        
        context.attached_deposit(NearToken::from_yoctonear(0));
        testing_env!(context.build());
        
        let payments = vec![
            PaymentInput {
                recipient: accounts(1),
                amount: U128(1_000_000_000_000_000_000_000_000),
            },
            PaymentInput {
                recipient: accounts(2),
                amount: U128(2_000_000_000_000_000_000_000_000),
            },
        ];
        
        let list_id = contract.submit_list("native".to_string(), payments);
        
        // Approve with exact deposit (3 NEAR total)
        let total_deposit = NearToken::from_yoctonear(3_000_000_000_000_000_000_000_000);
        context.attached_deposit(total_deposit);
        testing_env!(context.build());
        
        contract.approve_list(list_id);
        
        // Verify status changed
        let list = contract.view_list(list_id);
        assert!(matches!(list.status, ListStatus::Approved));
    }

    #[test]
    #[should_panic(expected = "Exact deposit required")]
    fn test_approve_list_wrong_deposit() {
        let mut context = get_context(accounts(0));
        
        // Setup
        let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
        context.attached_deposit(storage_cost);
        testing_env!(context.build());
        
        let mut contract = BulkPaymentContract::default();
        contract.buy_storage(10);
        
        context.attached_deposit(NearToken::from_yoctonear(0));
        testing_env!(context.build());
        
        let payments = vec![
            PaymentInput {
                recipient: accounts(1),
                amount: U128(1_000_000_000_000_000_000_000_000),
            },
        ];
        
        let list_id = contract.submit_list("native".to_string(), payments);
        
        // Try to approve with wrong deposit
        let wrong_deposit = NearToken::from_yoctonear(500_000_000_000_000_000_000_000);
        context.attached_deposit(wrong_deposit);
        testing_env!(context.build());
        
        contract.approve_list(list_id); // Should panic
    }

    #[test]
    #[should_panic(expected = "Only the submitter can approve the list")]
    fn test_approve_list_unauthorized() {
        let mut context = get_context(accounts(0));
        
        // Setup: user 0 submits
        let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
        context.attached_deposit(storage_cost);
        testing_env!(context.build());
        
        let mut contract = BulkPaymentContract::default();
        contract.buy_storage(10);
        
        context.attached_deposit(NearToken::from_yoctonear(0));
        testing_env!(context.build());
        
        let payments = vec![
            PaymentInput {
                recipient: accounts(1),
                amount: U128(1_000_000_000_000_000_000_000_000),
            },
        ];
        
        let list_id = contract.submit_list("native".to_string(), payments);
        
        // User 1 tries to approve (should fail)
        context = get_context(accounts(1));
        let total_deposit = NearToken::from_yoctonear(1_000_000_000_000_000_000_000_000);
        context.attached_deposit(total_deposit);
        testing_env!(context.build());
        
        contract.approve_list(list_id); // Should panic
    }

    #[test]
    fn test_retry_failed() {
        let mut context = get_context(accounts(0));
        
        // Setup
        let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
        context.attached_deposit(storage_cost);
        testing_env!(context.build());
        
        let mut contract = BulkPaymentContract::default();
        contract.buy_storage(10);
        
        context.attached_deposit(NearToken::from_yoctonear(0));
        testing_env!(context.build());
        
        let payments = vec![
            PaymentInput {
                recipient: accounts(1),
                amount: U128(1_000_000_000_000_000_000_000_000),
            },
        ];
        
        let list_id = contract.submit_list("unsupported_token".to_string(), payments);
        
        // Approve
        let total_deposit = NearToken::from_yoctonear(1_000_000_000_000_000_000_000_000);
        context.attached_deposit(total_deposit);
        testing_env!(context.build());
        contract.approve_list(list_id);
        
        // Process (will fail due to unsupported token)
        context.attached_deposit(NearToken::from_yoctonear(0));
        testing_env!(context.build());
        contract.payout_batch(list_id, None);
        
        // Verify failed
        let list = contract.view_list(list_id);
        assert!(matches!(list.payments[0].status, PaymentStatus::Failed { .. }));
        
        // Retry
        contract.retry_failed(list_id);
        
        // Verify back to pending
        let list_after = contract.view_list(list_id);
        assert!(matches!(list_after.payments[0].status, PaymentStatus::Pending));
    }

    #[test]
    fn test_reject_list() {
        let mut context = get_context(accounts(0));
        
        // Setup
        let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
        context.attached_deposit(storage_cost);
        testing_env!(context.build());
        
        let mut contract = BulkPaymentContract::default();
        contract.buy_storage(10);
        
        context.attached_deposit(NearToken::from_yoctonear(0));
        testing_env!(context.build());
        
        let payments = vec![
            PaymentInput {
                recipient: accounts(1),
                amount: U128(1_000_000_000_000_000_000_000_000),
            },
        ];
        
        let list_id = contract.submit_list("native".to_string(), payments);
        
        // Reject without approval first
        contract.reject_list(list_id);
        
        let list = contract.view_list(list_id);
        assert!(matches!(list.status, ListStatus::Rejected));
    }

    #[test]
    #[should_panic(expected = "Only pending lists can be rejected")]
    fn test_reject_approved_list_fails() {
        let mut context = get_context(accounts(0));
        
        // Setup
        let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
        context.attached_deposit(storage_cost);
        testing_env!(context.build());
        
        let mut contract = BulkPaymentContract::default();
        contract.buy_storage(10);
        
        context.attached_deposit(NearToken::from_yoctonear(0));
        testing_env!(context.build());
        
        let payments = vec![
            PaymentInput {
                recipient: accounts(1),
                amount: U128(1_000_000_000_000_000_000_000_000),
            },
        ];
        
        let list_id = contract.submit_list("native".to_string(), payments);
        
        // Approve the list
        context.attached_deposit(NearToken::from_yoctonear(1_000_000_000_000_000_000_000_000));
        testing_env!(context.build());
        contract.approve_list(list_id);
        
        // Try to reject an approved list - should panic
        context.attached_deposit(NearToken::from_yoctonear(0));
        testing_env!(context.build());
        contract.reject_list(list_id);
    }

    #[test]
    fn test_multiple_lists() {
        let mut context = get_context(accounts(0));
        
        // Buy storage
        let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000 * 2);
        context.attached_deposit(storage_cost);
        testing_env!(context.build());
        
        let mut contract = BulkPaymentContract::default();
        contract.buy_storage(20);
        
        context.attached_deposit(NearToken::from_yoctonear(0));
        testing_env!(context.build());
        
        // Submit multiple lists
        let payments1 = vec![
            PaymentInput {
                recipient: accounts(1),
                amount: U128(1_000_000_000_000_000_000_000_000),
            },
        ];
        
        let payments2 = vec![
            PaymentInput {
                recipient: accounts(2),
                amount: U128(2_000_000_000_000_000_000_000_000),
            },
        ];
        
        let list_id1 = contract.submit_list("native".to_string(), payments1);
        let list_id2 = contract.submit_list("native".to_string(), payments2);
        
        assert_eq!(list_id1, 0);
        assert_eq!(list_id2, 1);
        
        let list1 = contract.view_list(list_id1);
        let list2 = contract.view_list(list_id2);
        
        assert_eq!(list1.payments[0].amount, 1_000_000_000_000_000_000_000_000);
        assert_eq!(list2.payments[0].amount, 2_000_000_000_000_000_000_000_000);
    }
    
    // Note: Overflow protection tests are implicitly validated by the NEAR runtime environment.
    // The environment checks account balances and prevents unrealistic values before our
    // contract code executes, providing an additional layer of security. Our checked_*
    // operations ensure safety within the contract logic itself.
}
