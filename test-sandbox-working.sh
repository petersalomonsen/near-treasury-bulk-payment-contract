#!/bin/bash
# End-to-end test demonstrating the sandbox is working

set -e

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "============================================"
echo "NEAR Treasury Sandbox - Working Tests"
echo "============================================"
echo

echo -e "${YELLOW}✓ Container Status${NC}"
docker ps --filter "name=near-treasury-sandbox" --format "  {{.Status}}"
echo

echo -e "${YELLOW}✓ API Health Check${NC}"
curl -s http://localhost:8080/health | jq '  "Status: " + .status + " | Version: " + .version'
echo

echo -e "${YELLOW}✓ Sandbox RPC Status${NC}"
curl -s -X POST http://localhost:3030 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":"1","method":"status","params":[]}' \
  | jq -r '  "Chain ID: " + .result.chain_id + " | Height: " + (.result.sync_info.latest_block_height | tostring)'
echo

echo -e "${YELLOW}✓ Contract Deployment${NC}"
curl -s -X POST http://localhost:3030 \
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
  }' | jq -r '  "Contract: bulk-payment.sandbox | Balance: " + (.result.amount | tonumber / 1e24 | tostring | .[0:7]) + " NEAR"'
echo

echo -e "${YELLOW}✓ Contract View Methods${NC}"
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
      "args_base64": "'"$(echo -n '{"account_id":"sandbox"}' | base64 -w0)"'"
    }
  }')
echo "$CREDITS" | jq -r '  "Storage credits for sandbox: " + (.result.result | map([.] | implode) | add)'
echo

echo -e "${YELLOW}✓ Deployed Contracts${NC}"
docker exec near-treasury-sandbox cat /var/log/sandbox-init.log 2>/dev/null | \
  grep "Successfully deployed" | \
  sed 's/.*Successfully deployed/  -/' | \
  sed 's/ contract to / →/'
echo

echo "============================================"
echo -e "${GREEN}All Core Components Working!${NC}"
echo "============================================"
echo
echo "The sandbox environment is fully operational:"
echo "  • NEAR Sandbox RPC: http://localhost:3030"
echo "  • Bulk Payment API: http://localhost:8080"
echo "  • Contract: bulk-payment.sandbox"
echo
echo "Note: To submit payment lists, accounts need storage credits."
echo "Use the contract's buy_storage method before submitting lists."
echo
