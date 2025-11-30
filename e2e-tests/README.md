# E2E Tests for DAO Bulk Payment Flow

This directory contains end-to-end tests that demonstrate the full workflow for bulk payments from a DAO's perspective.

## Overview

The test script (`dao-bulk-payment-flow.js`) performs the following steps:

1. **Create a Sputnik DAO** (`testdao.sputnik-dao.near`)
2. **Buy Storage Proposal** - Create and approve a proposal to call `buy_storage` in the bulk payment contract
3. **Submit Payment List** - Call the bulk payment API to submit a list of 500 recipients
4. **Approve Payment List** - Create and approve a proposal to call `approve_list` with the required deposit
5. **Wait for Payouts** - The background worker processes approved lists automatically
6. **Verify Recipients** - Check that all recipients received their tokens

## Prerequisites

- Node.js 20+
- Docker (for local testing)
- Access to a running sandbox environment

## Configuration

The test can be configured via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `SANDBOX_RPC_URL` | `http://localhost:3030` | URL of the NEAR sandbox RPC |
| `API_URL` | `http://localhost:8080` | URL of the bulk payment API |
| `DAO_FACTORY_ID` | `sputnik-dao.near` | Sputnik DAO factory account |
| `BULK_PAYMENT_CONTRACT_ID` | `bulk-payment.sandbox` | Bulk payment contract account |
| `NUM_RECIPIENTS` | `500` | Number of payment recipients |
| `PAYMENT_AMOUNT` | `100000000000000000000000` | Amount per recipient (0.1 NEAR) |

## Running Tests

### Against Local Docker Container

```bash
# Build and start the Docker container
docker build -t near-treasury-sandbox -f sandbox/Dockerfile .
docker run -d --name sandbox -p 3030:3030 -p 8080:8080 -p 5001:5001 near-treasury-sandbox

# Wait for services to start
sleep 30

# Run the test
cd e2e-tests
npm install
npm run test:docker
```

### Against Fly.io Deployment

```bash
cd e2e-tests
npm install

# Set environment variables for your Fly.io deployment
export SANDBOX_RPC_URL=https://your-app.fly.dev:3030
export API_URL=https://your-app.fly.dev:8080

npm run test:fly
```

### GitHub Actions

The test runs automatically via GitHub Actions on:
- Push to `main` or `staging` branches
- Pull requests to `main` or `staging` branches
- Manual workflow dispatch (with optional Fly.io URL)

## Test Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           E2E Test Flow                                  │
└─────────────────────────────────────────────────────────────────────────┘

     ┌──────────────┐
     │ Genesis      │
     │ Account      │
     └──────┬───────┘
            │
            ▼
┌───────────────────────────────┐
│ 1. Create DAO                 │
│    testdao.sputnik-dao.near   │
└──────────────┬────────────────┘
               │
               ▼
┌───────────────────────────────┐
│ 2. Proposal: buy_storage      │
│    - 500 records              │
│    - Attached deposit         │
└──────────────┬────────────────┘
               │
               ▼
┌───────────────────────────────┐
│ 3. Approve buy_storage        │
│    (VoteApprove)              │
└──────────────┬────────────────┘
               │
               ▼
┌───────────────────────────────┐       ┌─────────────────────┐
│ 4. Submit payment list        │──────▶│ Bulk Payment API    │
│    POST /submit-list          │       │ - Validates list    │
│    - 500 recipients           │       │ - Returns list_id   │
└──────────────┬────────────────┘       └─────────────────────┘
               │
               ▼
┌───────────────────────────────┐
│ 5. Proposal: approve_list     │
│    - list_ref: <list_id>      │
│    - Attached deposit: 50 NEAR│
└──────────────┬────────────────┘
               │
               ▼
┌───────────────────────────────┐
│ 6. Approve list proposal      │
│    (VoteApprove)              │
└──────────────┬────────────────┘
               │
               ▼
┌───────────────────────────────┐       ┌─────────────────────┐
│ 7. Background Worker          │◀──────│ Bulk Payment API    │
│    - Polls every 5s           │       │ Worker Thread       │
│    - Calls payout_batch       │       └─────────────────────┘
│    - 100 payments per batch   │
└──────────────┬────────────────┘
               │
               ▼
┌───────────────────────────────┐
│ 8. Verify recipients          │
│    - Check balances           │
│    - All 500 received 0.1 NEAR│
└───────────────────────────────┘
```

## Files

- `package.json` - npm package configuration
- `dao-bulk-payment-flow.js` - Main test script
- `README.md` - This documentation

## Related

- [Sandbox Environment](../sandbox/README.md) - Docker deployment configuration
- [Bulk Payment API](../bulk-payment-api/README.md) - REST API documentation
- [Bulk Payment Contract](../src/lib.rs) - Smart contract implementation
