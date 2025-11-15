// NEAR Treasury Bulk Payment Contract
// See: https://github.com/NEAR-DevHub/near-treasury/issues/101
use near_sdk::{near, env, AccountId, NearToken, Promise};
use near_sdk::collections::UnorderedMap;
use near_sdk::borsh::{BorshDeserialize, BorshSerialize};

#[near(contract_state)]
pub struct BulkPaymentContract {
    payment_lists: UnorderedMap<u64, PaymentList>,
    storage_credits: UnorderedMap<AccountId, NearToken>,
    next_list_id: u64,
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
pub struct PaymentList {
    pub token_id: String,
    pub submitter: AccountId,
    pub status: ListStatus,
    pub payments: Vec<PaymentRecord>,
    pub created_at: u64,
}

#[near(serializers = [json, borsh])]
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
        }
    }
}

#[near]
impl BulkPaymentContract {
    // TODO: Implement buy_storage function
    // TODO: Implement submit_list function
    // TODO: Implement approve_list function
    // TODO: Implement payout_batch function
    // TODO: Implement reject_list function
    // TODO: Implement view_list function
    // TODO: Implement retry_failed function
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
