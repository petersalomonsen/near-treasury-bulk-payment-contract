# Integration Tests

This directory contains end-to-end integration tests using `near-workspaces`.

## Requirements

The integration tests require:
- Internet access to download the NEAR sandbox binary
- `near-workspaces` crate with dependencies (`tokio`, `serde_json`)

## Running Integration Tests

To run the integration tests, you need to add the required dependencies back to `Cargo.toml`:

```toml
[dev-dependencies]
near-sdk = { version = "5.16", features = ["unit-testing"] }
near-workspaces = { version = "0.21", features = ["unstable"] }
tokio = { version = "1.12.0", features = ["full"] }
serde_json = "1"
```

Then build the WASM and run tests:

```bash
# Build the contract WASM
cargo build --target wasm32-unknown-unknown --release

# Run all tests including integration tests
cargo test
```

## Test Coverage

The integration tests cover:

1. **Storage Purchase Test**: Verifies storage cost calculation with 10% markup
2. **Submit and Approve List Test**: Tests list submission and approval flow
3. **Batch Processing Test**: Tests processing 250 payments in batches of 100
4. **Failed Payment Retry Test**: Tests retry mechanism for failed payments
5. **Reject List with Refund Test**: Tests list rejection and deposit refund
6. **Revenue Generation Test**: Verifies contract generates profit from storage markup
7. **Exact Deposit Validation Test**: Tests deposit amount validation
8. **Unauthorized Operations Test**: Tests authorization checks

## Unit Tests

Unit tests are included in `src/lib.rs` and can be run without external dependencies:

```bash
cargo test --lib
```

These tests cover:
- Storage cost calculation
- Payment list submission and approval
- Authorization and access control
- Failed payment retry
- Multiple payment lists
- Error cases and edge conditions
