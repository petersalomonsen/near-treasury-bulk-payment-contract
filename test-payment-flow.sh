#!/bin/bash
# Test the complete payment flow in the sandbox

set -e

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

RPC_URL="http://localhost:3030"
CONTRACT_ID="bulk-payment.sandbox"

echo "============================================"
echo "Testing Payment Flow"
echo "============================================"
echo

# Step 1: Buy storage credits for the sandbox account
echo -e "${YELLOW}Step 1: Buying storage credits (10 NEAR)...${NC}"
BUY_STORAGE=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d "{
    \"jsonrpc\": \"2.0\",
    \"id\": \"buy-storage\",
    \"method\": \"broadcast_tx_commit\",
    \"params\": [
      \"$(echo -n '{"signer_id":"sandbox","receiver_id":"'$CONTRACT_ID'","actions":[{"type":"FunctionCall","params":{"method_name":"buy_storage","args":"{}","gas":30000000000000,"deposit":"10000000000000000000000000"}}]}' | base64 -w0)\"
    ]
  }")

if echo "$BUY_STORAGE" | jq -e '.result' > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Storage credits purchased${NC}"
else
    echo -e "${RED}✗ Failed to buy storage${NC}"
    echo "$BUY_STORAGE" | jq .
    exit 1
fi
echo

# Step 2: Check storage credits
echo -e "${YELLOW}Step 2: Checking storage credits...${NC}"
CREDITS=$(curl -s -X POST "$RPC_URL" \
  -H "Content-Type: application/json" \
  -d "{
    \"jsonrpc\": \"2.0\",
    \"id\": \"1\",
    \"method\": \"query\",
    \"params\": {
      \"request_type\": \"call_function\",
      \"finality\": \"final\",
      \"account_id\": \"$CONTRACT_ID\",
      \"method_name\": \"view_storage_credits\",
      \"args_base64\": \"$(echo -n '{"account_id":"sandbox"}' | base64 -w0)\"
    }
  }")

if echo "$CREDITS" | jq -e '.result.result' > /dev/null 2>&1; then
    RESULT_BYTES=$(echo "$CREDITS" | jq -r '.result.result | map([.] | implode) | add')
    echo -e "${GREEN}✓ Storage credits: $RESULT_BYTES${NC}"
else
    echo -e "${RED}✗ Failed to check credits${NC}"
fi
echo

# Step 3: Submit a payment list via the API
echo -e "${YELLOW}Step 3: Submitting payment list via API...${NC}"
SUBMIT=$(curl -s -X POST http://localhost:8080/submit-list \
  -H "Content-Type: application/json" \
  -d '{
    "submitter_id": "sandbox",
    "token_id": "native",
    "payments": [
      {"recipient": "alice.test.near", "amount": "1000000000000000000000000"},
      {"recipient": "bob.test.near", "amount": "2000000000000000000000000"}
    ]
  }')

if echo "$SUBMIT" | jq -e '.success' > /dev/null 2>&1; then
    LIST_ID=$(echo "$SUBMIT" | jq -r '.list_id')
    echo -e "${GREEN}✓ Payment list submitted with ID: $LIST_ID${NC}"
else
    echo -e "${RED}✗ Failed to submit payment list${NC}"
    echo "$SUBMIT" | jq .
    exit 1
fi
echo

# Step 4: View the payment list
echo -e "${YELLOW}Step 4: Viewing payment list $LIST_ID...${NC}"
VIEW=$(curl -s http://localhost:8080/list/$LIST_ID)

if echo "$VIEW" | jq -e '.list' > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Payment list retrieved${NC}"
    echo "$VIEW" | jq .
else
    echo -e "${RED}✗ Failed to view list${NC}"
    echo "$VIEW"
fi
echo

echo "============================================"
echo -e "${GREEN}Payment Flow Test Complete!${NC}"
echo "============================================"
