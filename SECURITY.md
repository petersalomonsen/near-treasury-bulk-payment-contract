# Security Considerations

## Overview

This document outlines the security measures implemented in the NEAR Treasury Bulk Payment Contract to protect against common smart contract vulnerabilities.

## Security Features

### 1. Arithmetic Overflow Protection

All arithmetic operations that could potentially overflow are protected with `checked_*` operations:

- **Storage cost calculation** (buy_storage):
  - `checked_mul` for bytes calculation
  - `checked_mul` for storage cost
  - `checked_mul` and `checked_div` for markup calculation
  - `checked_add` for credit accumulation

- **Payment amount totals** (approve_list):
  - `try_fold` with `checked_add` to sum payment amounts
  - Prevents overflow when calculating total deposit required

These protections ensure the contract will panic rather than silently overflow, preventing incorrect calculations that could lead to loss of funds.

### 2. Authorization Controls

All sensitive operations verify the caller's identity:

- **approve_list**: Only the list submitter can approve
- **reject_list**: Only the list submitter can reject
- **retry_failed**: Only the list submitter can retry failed payments

Unauthorized access attempts will panic with a clear error message.

### 3. Exact Deposit Validation

Functions requiring deposits validate exact amounts:

- **buy_storage**: Requires exact storage cost (prevents overpayment)
- **approve_list**: Requires exact total payment amount (prevents under/overpayment)

This prevents:
- Users accidentally sending too much NEAR
- Attackers attempting to exploit deposit mismatches
- Calculation errors leading to incorrect deposits

### 4. State Update Before External Calls

The contract follows the checks-effects-interactions pattern:

In `reject_list`:
1. Check authorization and status
2. Update state (mark as rejected, remove deposit tracking)
3. Make external call (transfer refund)

This prevents reentrancy attacks where an external contract could call back during execution.

### 5. Status Validation

Functions verify the current state before proceeding:

- **approve_list**: List must be Pending
- **payout_batch**: List must be Approved
- **retry_failed**: List must be Approved
- **reject_list**: List must not already be Rejected

These checks prevent:
- Double approval/rejection
- Processing unapproved lists
- Invalid state transitions

### 6. Input Validation

All inputs are validated:

- **buy_storage**: Requires num_records > 0
- **submit_list**: Requires non-empty payment list
- **submit_list**: Verifies sufficient storage credits
- List reference validation in all functions

### 7. Safe Numeric Conversions

Type conversions are done carefully:
- `u64` to `u128` conversions are always safe (widening)
- Result types are checked after arithmetic operations

### 8. Error Messages

All failures include descriptive error messages to help diagnose issues:
- "Exact deposit required: X, attached: Y"
- "Only the submitter can approve the list"
- "Insufficient storage credits. Required: X, Available: Y"

## Potential Risks and Mitigations

### 1. Cross-Contract Call Failures

**Risk**: Calls to `intents.near` for ft_withdraw could fail.

**Mitigation**: 
- Payment status tracks failures with error messages
- `retry_failed` function allows reprocessing
- Each payment is independent (one failure doesn't affect others)

**Future Enhancement**: Implement callbacks to verify success/failure of external calls.

### 2. Gas Limitations

**Risk**: Processing large payment batches could run out of gas.

**Mitigation**:
- Batch size limited to 100 payments per call
- Anyone can call `payout_batch` (not just submitter)
- Multiple calls can process large lists incrementally

### 3. Storage Costs

**Risk**: Large payment lists consume significant storage.

**Mitigation**:
- Storage credit system requires upfront payment
- 10% markup ensures contract sustainability
- Users pay proportional to usage

### 4. Refund Failures

**Risk**: Transfer in `reject_list` could fail to invalid recipients.

**Mitigation**:
- State is updated before transfer (no rollback needed)
- List is marked as rejected regardless of transfer success
- Deposit tracking is removed to prevent double refund

## Audit Recommendations

For production deployment, recommend:

1. **Professional audit** of arithmetic operations and overflow handling
2. **Gas profiling** for batch processing with various list sizes
3. **Integration testing** with actual intents.near contract on testnet
4. **Callback implementation** for cross-contract call verification
5. **Monitoring** of contract balance growth to verify revenue model
6. **Rate limiting** considerations for spam prevention

## Testing

All security features are covered by unit tests:
- Overflow scenarios
- Authorization checks
- Exact deposit validation
- State transition validation
- Multiple caller scenarios

Run tests with: `cargo test --lib`

## Responsible Disclosure

If you discover a security vulnerability, please email the repository maintainer directly rather than opening a public issue.
