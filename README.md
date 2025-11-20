# NEAR Treasury Bulk Payment Contract

NEAR smart contract for bulk payment processing - Part of NEAR Treasury

## Overview

This contract enables efficient batch payment processing on NEAR with support for:
- Native NEAR tokens
- NEP-141 fungible tokens via NEAR Intents (intents.near)
- Storage-based fee model with 10% revenue margin
- Batch processing of up to 100 payments at a time
- Payment status tracking and retry mechanism

## Key Features

### 1. Storage Credit System
- Users purchase storage credits to submit payment lists
- 10% markup on actual NEAR storage costs generates contract revenue
- Storage cost: 216 bytes per payment record
- Exact deposit amount required (prevents overpayment)

### 2. Payment List Management
- Submit lists with any number of payments
- Approve lists with exact deposit matching total payment amount
- Process payments in batches (max 100 per call)
- Reject lists with automatic deposit refund

### 3. NEAR Intents Integration
- Supports NEAR Intents for fungible token transfers
- Token format: `nep141:<token_contract>` (e.g., `nep141:wrap.near`)
- Calls `ft_withdraw` on `intents.near` contract
- Native NEAR transfers also supported with `token_id: "native"`

### 4. Payment Status Tracking
- **Pending**: Payment not yet processed
- **Paid**: Payment successfully completed
- **Failed**: Payment failed with error message

### 5. List Status Management
- **Pending**: List submitted but not approved
- **Approved**: List approved and ready for processing
- **Rejected**: List rejected, deposits refunded

## Contract Functions

### buy_storage(num_records: u64) -> NearToken
Purchases storage credits for payment records.
- Calculates cost with 10% markup
- Requires exact deposit amount
- Returns total cost paid

### submit_list(token_id: String, payments: Vec<PaymentInput>) -> u64
Submits a new payment list.
- Verifies sufficient storage credits
- Deducts credits based on number of payments
- Returns list reference ID

### approve_list(list_ref: u64)
Approves a payment list for processing.
- Only submitter can approve
- Requires exact deposit matching total payment amount
- Changes status to Approved

### payout_batch(list_ref: u64, max_payments: Option<u64>)
Processes payments in batches (public function).
- Processes up to 100 payments per call (configurable)
- For NEAR Intents: calls ft_withdraw on intents.near
- For native NEAR: direct transfer
- Updates payment status to Paid or Failed

### reject_list(list_ref: u64)
Rejects a payment list.
- Only submitter can reject
- Refunds approval deposit if any
- Changes status to Rejected

### view_list(list_ref: u64) -> PaymentList
Views payment list details including all payment statuses.

### retry_failed(list_ref: u64)
Resets failed payments to pending status.
- Only submitter can retry
- List must be Approved
- Only affects Failed payments

### view_storage_credits(account_id: AccountId) -> NearToken
Views storage credits for an account.

## Building

```bash
# Check the contract compiles
cargo check --target wasm32-unknown-unknown

# Build the WASM binary
cargo build --target wasm32-unknown-unknown --release

# WASM output location
# target/wasm32-unknown-unknown/release/near_treasury_bulk_payment_contract.wasm
```

## Testing

```bash
# Run unit tests (no external dependencies)
cargo test --lib

# Run all tests including integration tests (requires internet access)
# See tests/README.md for setup instructions
cargo test
```

## Usage Example

```rust
// 1. Buy storage credits
let storage_cost = contract.buy_storage(10);

// 2. Submit payment list
let payments = vec![
    PaymentInput {
        recipient: "alice.near".parse().unwrap(),
        amount: U128(1_000_000_000_000_000_000_000_000), // 1 NEAR
    },
    PaymentInput {
        recipient: "bob.near".parse().unwrap(),
        amount: U128(2_000_000_000_000_000_000_000_000), // 2 NEAR
    },
];

let list_id = contract.submit_list("native".to_string(), payments);

// 3. Approve with exact deposit (3 NEAR total)
contract.approve_list(list_id);

// 4. Process payments (anyone can call)
contract.payout_batch(list_id, Some(100));

// 5. View list status
let list = contract.view_list(list_id);
```

## Security Features

- Exact deposit validation (prevents overpayment/underpayment)
- Authorization checks (only submitter can approve/reject/retry)
- Safe arithmetic (no overflow risks)
- Failed payment tracking and retry mechanism
- Cross-contract call error handling

## Revenue Model

The contract generates revenue from storage purchases:
- Base cost: Actual NEAR storage cost (10^19 yoctoNEAR per byte)
- Markup: 10% added to base cost
- Revenue stays in contract and grows with usage

Example: 10 records = 2,160 bytes
- Base cost: 21.6 NEAR
- With 10% markup: 23.76 NEAR
- Revenue: 2.16 NEAR per purchase

## License

See LICENSE file for details.
