# Integration Tests

This directory contains end-to-end integration tests using `near-sandbox` and `near-api`.

## Requirements

The integration tests require:
- System libraries for `hidapi` (part of `near-api` dependencies):
  - On Ubuntu/Debian: `sudo apt-get install libudev-dev pkg-config`
  - On Fedora/RHEL: `sudo dnf install systemd-devel`
  - On macOS: Should work out of the box
- `near-sandbox`, `near-api`, `cargo-near-build` crates

## Running Integration Tests

The required dependencies are already in `Cargo.toml`:

```toml
[dev-dependencies]
near-sdk = { version = "5.16", features = ["unit-testing"] }
near-sandbox = "0.2.0"
near-api = "0.7.7"
cargo-near-build = "0.8.0"
tokio = { version = "1.12.0", features = ["full"] }
serde_json = "1"
```

Install system dependencies first (on Ubuntu/Debian):

```bash
sudo apt-get install libudev-dev pkg-config
```

Then run tests:

```bash
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
