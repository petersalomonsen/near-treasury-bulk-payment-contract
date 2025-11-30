# Bulk Payment API

REST API service for interacting with the NEAR Treasury Bulk Payment Contract.

## Overview

This service provides a simple HTTP interface for:
- Submitting payment lists to the bulk payment contract
- Querying payment list status
- Automatic processing of approved lists via a background worker

## Endpoints

### Health Check

```
GET /health
```

Returns the service health status.

**Response:**
```json
{
  "status": "healthy",
  "service": "bulk-payment-api",
  "version": "0.1.0"
}
```

### Submit Payment List

```
POST /submit-list
```

Submit a new payment list to the bulk payment contract.

**Request Body:**
```json
{
  "submitter_id": "user.test.near",
  "token_id": "native",
  "payments": [
    {"recipient": "alice.test.near", "amount": "1000000000000000000000000"},
    {"recipient": "bob.test.near", "amount": "2000000000000000000000000"}
  ]
}
```

**Response:**
```json
{
  "success": true,
  "list_id": 0,
  "error": null
}
```

### Get Payment List

```
GET /list/{id}
```

Get the status and details of a payment list.

**Response:**
```json
{
  "success": true,
  "list": {
    "id": 0,
    "token_id": "native",
    "submitter": "user.test.near",
    "status": "Approved",
    "total_payments": 2,
    "pending_payments": 0,
    "paid_payments": 2,
    "failed_payments": 0,
    "created_at": 1234567890
  },
  "error": null
}
```

## Configuration

The service is configured via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `NEAR_RPC_URL` | `http://localhost:3030` | URL of the NEAR RPC endpoint |
| `BULK_PAYMENT_CONTRACT_ID` | `bulk-payment.test.near` | Contract account ID |
| `API_PORT` | `8080` | Port to listen on |
| `WORKER_CALLER_ID` | `test.near` | Account ID for the worker to use |

## Background Worker

The service includes a background worker that:
1. Polls for approved payment lists every 5 seconds
2. Calls `payout_batch` with up to 100 payments per call
3. Continues until all payments in a list are processed
4. Removes completed lists from the processing queue

## Building

```bash
cd bulk-payment-api
cargo build --release
```

## Running

```bash
# With default configuration
./target/release/bulk-payment-api

# With custom configuration
NEAR_RPC_URL=http://localhost:3030 \
BULK_PAYMENT_CONTRACT_ID=bulk-payment.test.near \
API_PORT=8080 \
./target/release/bulk-payment-api
```

## Dependencies

- `axum` - Web framework
- `tokio` - Async runtime
- `near-api` - NEAR RPC client
- `serde` - Serialization
- `tracing` - Logging

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                  Bulk Payment API                    │
│                                                      │
│  ┌──────────────┐        ┌──────────────────────┐   │
│  │  HTTP Server │        │  Background Worker   │   │
│  │   (axum)     │        │                      │   │
│  │              │        │  - Polls every 5s    │   │
│  │  /submit-list│◄──────►│  - Calls payout_batch│   │
│  │  /list/{id}  │        │  - Tracks progress   │   │
│  │  /health     │        │                      │   │
│  └──────────────┘        └──────────────────────┘   │
│         │                         │                  │
│         └─────────┬───────────────┘                  │
│                   │                                  │
│           ┌───────▼───────┐                         │
│           │ BulkPayment   │                         │
│           │    Client     │                         │
│           └───────┬───────┘                         │
│                   │                                  │
└───────────────────┼──────────────────────────────────┘
                    │
                    ▼
         ┌─────────────────────┐
         │   NEAR Sandbox      │
         │   (RPC :3030)       │
         │                     │
         │ bulk-payment.test.near │
         └─────────────────────┘
```
