#!/bin/bash
# Initialize NEAR Sandbox with pre-deployed contracts
# This script runs after near-sandbox starts

set -e

NEAR_RPC_URL="${NEAR_RPC_URL:-http://localhost:3030}"
CONTRACTS_DIR="/app/contracts"
DATA_DIR="/data"
INIT_MARKER="$DATA_DIR/.initialized"

# Wait for near-sandbox to be ready
wait_for_sandbox() {
    echo "Waiting for NEAR sandbox to be ready..."
    local max_attempts=30
    local attempt=0
    while [ $attempt -lt $max_attempts ]; do
        if curl -s "$NEAR_RPC_URL/status" > /dev/null 2>&1; then
            echo "NEAR sandbox is ready!"
            return 0
        fi
        attempt=$((attempt + 1))
        echo "Attempt $attempt/$max_attempts - sandbox not ready yet..."
        sleep 2
    done
    echo "ERROR: NEAR sandbox did not become ready in time"
    return 1
}

# Fetch contract from mainnet
fetch_contract_from_mainnet() {
    local account_id="$1"
    local output_path="$2"
    
    echo "Fetching contract from mainnet: $account_id"
    
    local response=$(curl -s -X POST https://rpc.mainnet.near.org \
        -H "Content-Type: application/json" \
        -d "{
            \"jsonrpc\": \"2.0\",
            \"id\": \"1\",
            \"method\": \"query\",
            \"params\": {
                \"request_type\": \"view_code\",
                \"finality\": \"final\",
                \"account_id\": \"$account_id\"
            }
        }")
    
    # Extract base64 code and decode
    local code_base64=$(echo "$response" | jq -r '.result.code_base64')
    if [ "$code_base64" = "null" ] || [ -z "$code_base64" ]; then
        echo "ERROR: Failed to fetch contract code for $account_id"
        return 1
    fi
    
    echo "$code_base64" | base64 -d > "$output_path"
    echo "Contract saved to $output_path ($(wc -c < "$output_path") bytes)"
}

# Create account using near-sandbox RPC
create_account() {
    local account_id="$1"
    local balance="${2:-100000000000000000000000000}"  # 100 NEAR default
    
    echo "Creating account: $account_id with balance $balance"
    
    # Use the sandbox RPC to create account
    # Note: In sandbox mode, we can use the test.near account as the creator
    curl -s -X POST "$NEAR_RPC_URL" \
        -H "Content-Type: application/json" \
        -d "{
            \"jsonrpc\": \"2.0\",
            \"id\": \"1\",
            \"method\": \"sandbox_patch_state\",
            \"params\": {
                \"records\": [
                    {
                        \"Account\": {
                            \"account_id\": \"$account_id\",
                            \"account\": {
                                \"amount\": \"$balance\",
                                \"locked\": \"0\",
                                \"code_hash\": \"11111111111111111111111111111111\",
                                \"storage_usage\": 0
                            }
                        }
                    }
                ]
            }
        }" > /dev/null
}

# Deploy contract to account
deploy_contract() {
    local account_id="$1"
    local wasm_path="$2"
    local init_args="${3:-}"
    local init_method="${4:-new}"
    
    echo "Deploying contract to $account_id from $wasm_path"
    
    # Read and base64 encode the WASM file
    local code_base64=$(base64 -w 0 "$wasm_path")
    
    # Patch state to deploy the contract code
    curl -s -X POST "$NEAR_RPC_URL" \
        -H "Content-Type: application/json" \
        -d "{
            \"jsonrpc\": \"2.0\",
            \"id\": \"1\",
            \"method\": \"sandbox_patch_state\",
            \"params\": {
                \"records\": [
                    {
                        \"Contract\": {
                            \"account_id\": \"$account_id\",
                            \"code\": \"$code_base64\"
                        }
                    }
                ]
            }
        }" > /dev/null
    
    echo "Contract deployed to $account_id"
}

# Main initialization logic
main() {
    # Check if already initialized
    if [ -f "$INIT_MARKER" ]; then
        echo "Sandbox already initialized, skipping..."
        exit 0
    fi
    
    wait_for_sandbox
    
    echo "================================================"
    echo "Initializing NEAR Sandbox with contracts..."
    echo "================================================"
    
    # Create contracts directory if needed
    mkdir -p "$CONTRACTS_DIR"
    
    # Fetch contracts from mainnet
    echo ""
    echo "Fetching contracts from mainnet..."
    
    if [ ! -f "$CONTRACTS_DIR/intents.wasm" ]; then
        fetch_contract_from_mainnet "intents.near" "$CONTRACTS_DIR/intents.wasm"
    fi
    
    if [ ! -f "$CONTRACTS_DIR/omft.wasm" ]; then
        fetch_contract_from_mainnet "omft.near" "$CONTRACTS_DIR/omft.wasm"
    fi
    
    if [ ! -f "$CONTRACTS_DIR/wrap.wasm" ]; then
        fetch_contract_from_mainnet "wrap.near" "$CONTRACTS_DIR/wrap.wasm"
    fi
    
    # Create accounts
    echo ""
    echo "Creating accounts..."
    
    create_account "intents.near"
    create_account "omft.near"
    create_account "wrap.near"
    create_account "dao.near"
    create_account "bulk-payment.test.near"
    
    # Deploy contracts
    echo ""
    echo "Deploying contracts..."
    
    deploy_contract "intents.near" "$CONTRACTS_DIR/intents.wasm"
    deploy_contract "omft.near" "$CONTRACTS_DIR/omft.wasm"
    deploy_contract "wrap.near" "$CONTRACTS_DIR/wrap.wasm"
    
    # Deploy bulk payment contract (built from this repo)
    if [ -f "$CONTRACTS_DIR/bulk_payment.wasm" ]; then
        deploy_contract "bulk-payment.test.near" "$CONTRACTS_DIR/bulk_payment.wasm"
    else
        echo "WARNING: bulk_payment.wasm not found, skipping deployment"
    fi
    
    # Deploy sample DAO contract
    if [ -f "$CONTRACTS_DIR/sputnikdao2.wasm" ]; then
        create_account "sample-dao.sputnik-dao.near"
        deploy_contract "sample-dao.sputnik-dao.near" "$CONTRACTS_DIR/sputnikdao2.wasm"
    else
        echo "WARNING: sputnikdao2.wasm not found, skipping DAO deployment"
    fi
    
    # Mark as initialized
    touch "$INIT_MARKER"
    
    echo ""
    echo "================================================"
    echo "Sandbox initialization complete!"
    echo "================================================"
    echo ""
    echo "Available contracts:"
    echo "  - intents.near"
    echo "  - omft.near"
    echo "  - wrap.near"
    echo "  - bulk-payment.test.near"
    echo "  - sample-dao.sputnik-dao.near (if sputnikdao2.wasm was provided)"
    echo ""
    echo "RPC URL: $NEAR_RPC_URL"
}

main "$@"
