# NEAR Treasury Sandbox Test Environment

This directory contains the configuration for deploying a comprehensive sandbox test environment on Fly.io. The environment includes all required services for testing the NEAR Treasury Bulk Payment system.

## Components

### 1. NEAR Sandbox (Port 3030)
- Local NEAR blockchain environment for testing
- Pre-deployed contracts on startup:
  - `bulk-payment.test.near` - Bulk payment contract (built from this repo)
  - `intents.near` - Imported from mainnet
  - `omft.near` - Imported from mainnet
  - `wrap.near` - Imported from mainnet
  - `sample-dao.sputnik-dao.near` - Sample DAO (optional)

### 2. Bulk Payment API (Port 8080)
- REST API for submitting and managing payment lists
- Background worker for automated payout processing
- Endpoints:
  - `POST /submit-list` - Submit a new payment list
  - `GET /list/{id}` - Get payment list status
  - `GET /health` - Health check

### 3. Sputnik DAO Indexer (Port 5001)
- Caching API server for SputnikDAO contracts
- Configured to point to the sandbox RPC instead of mainnet
- Provides proposal search, filtering, and voting discovery

## Deployment

### Prerequisites
- [Fly.io CLI](https://fly.io/docs/getting-started/installing-flyctl/) installed
- Fly.io account with billing enabled

### Deploy to Fly.io

1. **Create the Fly.io app** (first time only):
   ```bash
   cd sandbox
   fly apps create near-treasury-sandbox
   ```

2. **Create a persistent volume** (first time only):
   ```bash
   fly volumes create sandbox_data --size 10 --region ams
   ```

3. **Deploy**:
   ```bash
   fly deploy --config fly.toml
   ```

### Verify Deployment

Check that all services are running:

```bash
# Check NEAR Sandbox
curl https://near-treasury-sandbox.fly.dev:3030/status

# Check Bulk Payment API
curl https://near-treasury-sandbox.fly.dev:8080/health

# Check Sputnik Indexer
curl https://near-treasury-sandbox.fly.dev:5001/health
```

## Local Development

You can also run the sandbox environment locally using Docker:

```bash
# Build the image
docker build -f sandbox/Dockerfile -t near-treasury-sandbox .

# Run with persistent storage
docker run -d \
  --name near-treasury-sandbox \
  -p 3030:3030 \
  -p 8080:8080 \
  -p 5001:5001 \
  -v sandbox_data:/data \
  near-treasury-sandbox
```

## Usage

### Submit a Payment List

```bash
curl -X POST https://near-treasury-sandbox.fly.dev:8080/submit-list \
  -H "Content-Type: application/json" \
  -d '{
    "submitter_id": "user.test.near",
    "token_id": "native",
    "payments": [
      {"recipient": "alice.test.near", "amount": "1000000000000000000000000"},
      {"recipient": "bob.test.near", "amount": "2000000000000000000000000"}
    ]
  }'
```

### Get Payment List Status

```bash
curl https://near-treasury-sandbox.fly.dev:8080/list/0
```

### Query DAO Proposals (Indexer)

```bash
curl https://near-treasury-sandbox.fly.dev:5001/proposals/sample-dao.sputnik-dao.near
```

### Direct RPC Calls (Sandbox)

```bash
curl -X POST https://near-treasury-sandbox.fly.dev:3030 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": "1",
    "method": "query",
    "params": {
      "request_type": "view_account",
      "finality": "final",
      "account_id": "bulk-payment.test.near"
    }
  }'
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `NEAR_RPC_URL` | `http://localhost:3030` | URL of the NEAR sandbox RPC |
| `BULK_PAYMENT_CONTRACT_ID` | `bulk-payment.test.near` | Contract ID for bulk payments |
| `API_PORT` | `8080` | Port for the Bulk Payment API |
| `INDEXER_PORT` | `5001` | Port for the Sputnik Indexer |

### Adding Custom Contracts

To deploy additional contracts:

1. Add the WASM file to `sandbox/contracts/`
2. Modify `init-sandbox.sh` to create the account and deploy the contract

## Troubleshooting

### View Service Logs

```bash
# SSH into the machine
fly ssh console

# View logs
tail -f /var/log/near-sandbox.log
tail -f /var/log/bulk-payment-api.log
tail -f /var/log/sputnik-indexer.log
```

### Restart Services

```bash
fly ssh console
supervisorctl restart all
```

### Reset Sandbox State

```bash
fly ssh console
rm -f /data/.initialized
supervisorctl restart init-sandbox
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Fly.io Machine                           │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                   Supervisord                        │   │
│  │  ┌───────────────┐ ┌───────────────┐ ┌─────────────┐ │   │
│  │  │ near-sandbox  │ │bulk-payment-  │ │  sputnik-   │ │   │
│  │  │   :3030       │ │    api        │ │  indexer    │ │   │
│  │  │               │ │   :8080       │ │   :5001     │ │   │
│  │  └───────────────┘ └───────────────┘ └─────────────┘ │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              Persistent Volume (/data)               │   │
│  │   - Sandbox blockchain state                         │   │
│  │   - Indexer cache                                    │   │
│  │   - Initialization marker                            │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## Related Documentation

- [NEAR Sandbox Documentation](https://docs.near.org/tools/near-sandbox)
- [Bulk Payment Contract](../README.md)
- [Sputnik DAO Indexer](https://github.com/near-daos/sputnik-dao-caching-api-server)
- [Fly.io Documentation](https://fly.io/docs/)
