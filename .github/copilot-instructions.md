# GitHub Copilot Instructions for near-treasury-bulk-payment-contract

## Project Overview

This is a NEAR smart contract for bulk payment processing, part of the NEAR Treasury system. The contract allows for managing and executing bulk payments on the NEAR blockchain.

**Repository**: https://github.com/petersalomonsen/near-treasury-bulk-payment-contract
**Related Issue**: https://github.com/NEAR-DevHub/near-treasury/issues/101

## Technology Stack

- **Language**: Rust (Edition 2021)
- **Blockchain Platform**: NEAR Protocol
- **SDK**: near-sdk v5.16
- **Build Tool**: Cargo with cargo-near extension
- **Target**: WebAssembly (WASM)

## Architecture

The contract consists of:

- **BulkPaymentContract**: Main contract state with 3 fields:
  - `payment_lists`: IterableMap<u64, PaymentList> - Stores all payment lists
  - `storage_credits`: IterableMap<AccountId, NearToken> - Tracks storage credits per user
  - `next_list_id`: u64 - Auto-incrementing list ID counter
- **PaymentList**: Structure for organizing multiple payments with approval workflow
- **PaymentRecord**: Individual payment records with status tracking
- **PaymentStatus**: Enum for tracking payment states (Pending, Paid, Failed)
- **ListStatus**: Enum for tracking list approval states (Pending, Approved, Rejected)
- **MultiTokenReceiver**: NEP-245 trait implementation for mt_on_transfer callback

## Key Features (Fully Implemented)

- Storage credit management with 10% revenue markup (`buy_storage`)
- Payment list submission with credit deduction (`submit_list`)
- List approval via direct deposit or ft_on_transfer/mt_on_transfer callbacks (`approve_list`)
- Batch payment execution supporting native NEAR, NEP-141 tokens, and NEAR Intents (`payout_batch`)
- Failed payment retry mechanism (`retry_failed`)
- Payment list viewing (`view_list`, `view_storage_credits`)
- List rejection for pending lists only (`reject_list`)
- No approval deposit refunds (deposits managed by blockchain balance)

## Development Commands

### Build
```bash
cargo build --target wasm32-unknown-unknown --release
```

Or using cargo-near:
```bash
cargo near build
```

### Test
```bash
cargo test
```

### Lint
```bash
cargo clippy -- -D warnings
```

### Format
```bash
cargo fmt
```

## Code Style Guidelines

1. **Use NEAR SDK Macros**: Leverage `#[near]` macros for contract state, serialization, and methods
2. **Error Handling**: Use proper error types and messages for contract panics
3. **Gas Optimization**: Be mindful of gas costs, especially in loops and storage operations
4. **Storage Management**: Use efficient data structures (UnorderedMap, Vector) from near-sdk
5. **Security**: Validate inputs, check permissions, and handle edge cases
6. **Testing**: Write comprehensive unit tests for all contract methods
7. **Documentation**: Add doc comments for public methods and complex logic

## NEAR-Specific Considerations

- Always use `AccountId` type for account identifiers
- Use `NearToken` for token amounts (replaces deprecated Balance type)
- Contract methods should be annotated with proper visibility (`#[public]` if needed)
- Consider cross-contract calls and callback handling for complex operations
- Storage costs must be paid by users (implement storage deposits)
- Test with realistic gas limits and storage requirements

## Important Notes

- The contract uses reproducible builds with Docker image `sourcescan/cargo-near:0.16.2-rust-1.86.0`
- Rust toolchain version is pinned to 1.86.0 (see rust-toolchain.toml)
- The contract is a library crate (`crate-type = ["cdylib", "rlib"]`)
- Release builds are optimized for size (`opt-level = "z"`) with LTO enabled

## Development Workflow

1. Make code changes in `src/lib.rs`
2. Run tests: `cargo test`
3. Check formatting: `cargo fmt --check`
4. Run linter: `cargo clippy`
5. Build WASM: `cargo near build`
6. Commit changes with clear messages

## Testing Guidelines

- Write unit tests in the `tests` module within `lib.rs` (11 tests covering all features)
- Write integration tests in `tests/integration_tests.rs` (9 comprehensive end-to-end tests)
- Write E2E tests in `e2e-tests/` directory using JavaScript/Node.js
- Use `near-sdk` unit-testing features for contract testing
- Use `near-sandbox` and `near-api` for integration testing
- Test with random payment amounts to verify correct routing (not fixed amounts)
- For BTC intents tests: Verify exact burn event counts (100 mt_burn + 100 ft_burn = 200 total)
- For BTC intents tests: Validate per-event content (amount and recipient in each burn event)
- For NEAR/FT tests: Verify recipient balances directly (not burn events)
- Mock external calls and test error conditions
- Verify storage operations and gas usage
- Test edge cases around payment status transitions and authorization

### Hard Assertions Required

**All test verifications MUST use hard assertions (`assert`, `assert.equal`, `assert.ok`, `assert.fail`) - NEVER just `console.log` with if/else.** Tests must actually fail when expectations aren't met.

❌ **Wrong** - test passes even when expectation fails:
```javascript
if (balance >= expectedAmount) {
  console.log(`✅ Balance correct`);
} else {
  console.log(`❌ Balance wrong`);  // Just logs, test continues and passes!
}
```

✅ **Correct** - test fails when expectation fails:
```javascript
assert.ok(balance >= expectedAmount, `Balance ${balance} must be >= ${expectedAmount}`);
```

This applies to:
- Balance verifications (registered accounts must receive expected amounts)
- Zero-balance assertions (non-registered accounts must have 0 balance)
- Transaction success/failure outcomes (fail immediately on unexpected results)
- API response validations (don't silently skip errors)

## When Adding New Features

1. Follow the existing pattern for state management using IterableMap (not UnorderedMap)
2. Add appropriate serialization macros (`#[near(serializers = [json, borsh])]`)
3. Implement view methods (read-only) separately from state-changing methods
4. Document method parameters and return values
5. Handle all error cases explicitly with require! and proper error messages
6. Add corresponding unit tests AND integration tests
7. For payment tests: Use random amounts per recipient to verify correct routing
8. Update README.md, tests/README.md, and this file if adding significant functionality
