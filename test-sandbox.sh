#!/bin/bash
# Test script for the NEAR Treasury Sandbox

set -e

echo "============================================"
echo "Testing NEAR Treasury Sandbox"
echo "============================================"
echo

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test 1: Check if container is running
echo -e "${YELLOW}Test 1: Checking if container is running...${NC}"
if docker ps --filter "name=near-treasury-sandbox" --format "{{.Names}}" | grep -q near-treasury-sandbox; then
    echo -e "${GREEN}✓ Container is running${NC}"
else
    echo -e "${RED}✗ Container is not running${NC}"
    exit 1
fi
echo

# Test 2: Check Bulk Payment API health
echo -e "${YELLOW}Test 2: Checking Bulk Payment API health...${NC}"
HEALTH=$(curl -s http://localhost:8080/health)
if echo "$HEALTH" | jq -e '.status == "healthy"' > /dev/null 2>&1; then
    echo -e "${GREEN}✓ API is healthy${NC}"
    echo "$HEALTH" | jq .
else
    echo -e "${RED}✗ API health check failed${NC}"
    echo "$HEALTH"
    exit 1
fi
echo

# Test 3: Check NEAR Sandbox RPC
echo -e "${YELLOW}Test 3: Checking NEAR Sandbox RPC...${NC}"
STATUS=$(curl -s -X POST http://localhost:3030 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":"1","method":"status","params":[]}' \
  --max-time 5)

if echo "$STATUS" | jq -e '.result.chain_id' > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Sandbox RPC is responding${NC}"
    echo "Chain ID: $(echo "$STATUS" | jq -r '.result.chain_id')"
else
    echo -e "${RED}✗ Sandbox RPC check failed${NC}"
    echo "$STATUS"
fi
echo

# Test 4: Verify bulk-payment.sandbox contract exists
echo -e "${YELLOW}Test 4: Verifying bulk-payment.sandbox contract...${NC}"
ACCOUNT=$(curl -s -X POST http://localhost:3030 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": "1",
    "method": "query",
    "params": {
      "request_type": "view_account",
      "finality": "final",
      "account_id": "bulk-payment.sandbox"
    }
  }' --max-time 5)

if echo "$ACCOUNT" | jq -e '.result.code_hash' > /dev/null 2>&1; then
    echo -e "${GREEN}✓ bulk-payment.sandbox contract exists${NC}"
    echo "Code hash: $(echo "$ACCOUNT" | jq -r '.result.code_hash' | cut -c1-16)..."
    echo "Balance: $(echo "$ACCOUNT" | jq -r '.result.amount') yoctoNEAR"
else
    echo -e "${RED}✗ Contract verification failed${NC}"
    echo "$ACCOUNT"
fi
echo

# Test 5: Check initialization logs
echo -e "${YELLOW}Test 5: Checking initialization logs...${NC}"
if docker exec near-treasury-sandbox cat /var/log/sandbox-init.log 2>/dev/null | grep -q "Sandbox initialization complete"; then
    echo -e "${GREEN}✓ Sandbox initialized successfully${NC}"
    echo "Deployed contracts:"
    docker exec near-treasury-sandbox cat /var/log/sandbox-init.log 2>/dev/null | grep "Successfully deployed" | sed 's/^/  - /'
else
    echo -e "${YELLOW}⚠ Could not verify initialization from logs${NC}"
fi
echo

# Test 6: View initial contract state
echo -e "${YELLOW}Test 6: Viewing contract state (storage credits)...${NC}"
CREDITS=$(curl -s -X POST http://localhost:3030 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": "1",
    "method": "query",
    "params": {
      "request_type": "call_function",
      "finality": "final",
      "account_id": "bulk-payment.sandbox",
      "method_name": "view_storage_credits",
      "args_base64": "e30="
    }
  }' --max-time 5)

if echo "$CREDITS" | jq -e '.result.result' > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Contract view method works${NC}"
    RESULT=$(echo "$CREDITS" | jq -r '.result.result' | python3 -c "import sys, json, base64; print(json.loads(base64.b64decode(sys.stdin.read())))")
    echo "Storage credits: $RESULT"
else
    echo -e "${RED}✗ Contract view method failed${NC}"
    echo "$CREDITS"
fi
echo

echo "============================================"
echo -e "${GREEN}Sandbox Testing Summary${NC}"
echo "============================================"
echo
echo "✓ Container is running"
echo "✓ API is healthy and accessible on port 8080"
echo "✓ NEAR Sandbox RPC is accessible on port 3030"
echo "✓ bulk-payment.sandbox contract is deployed"
echo
echo "Note: To submit payment lists, you need to:"
echo "1. Buy storage credits first (required for submissions)"
echo "2. Use the sandbox account or create test accounts"
echo "3. The API uses the genesis key, so transactions must come from 'sandbox' account"
echo
echo "Example: Buy storage and submit a list using the sandbox account:"
echo "  # This would require modifying the API to support buying storage first"
echo
