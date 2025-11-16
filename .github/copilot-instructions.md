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

- **BulkPaymentContract**: Main contract state managing payment lists and storage credits
- **PaymentList**: Structure for organizing multiple payments with approval workflow
- **PaymentRecord**: Individual payment records with status tracking
- **PaymentStatus**: Enum for tracking payment states (Pending, Paid, Failed)
- **ListStatus**: Enum for tracking list approval states (Pending, Approved, Rejected)

## Key Features (To Be Implemented)

- Storage credit management (`buy_storage`)
- Payment list submission (`submit_list`)
- List approval workflow (`approve_list`, `reject_list`)
- Batch payment execution (`payout_batch`)
- Failed payment retry mechanism (`retry_failed`)
- Payment list viewing (`view_list`)

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

- Write unit tests in the `tests` module within `lib.rs`
- Use `near-sdk` unit-testing features for contract testing
- Test edge cases, especially around payment status transitions
- Mock external calls and test error conditions
- Verify storage operations and gas usage

## When Adding New Features

1. Follow the existing pattern for state management using UnorderedMap
2. Add appropriate serialization macros (`#[near(serializers = [json, borsh])]`)
3. Implement view methods (read-only) separately from state-changing methods
4. Document method parameters and return values
5. Handle all error cases explicitly
6. Add corresponding unit tests
7. Update this documentation if adding significant functionality
