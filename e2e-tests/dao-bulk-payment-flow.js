/**
 * End-to-End Test: DAO Bulk Payment Flow
 * 
 * This script demonstrates the full workflow for bulk payments from a DAO's perspective:
 * 1. Create a Sputnik DAO instance (testdao.sputnik-dao.near)
 * 2. Create a proposal to buy_storage in the bulk payment contract
 * 3. Approve the buy_storage proposal
 * 4. Submit a payment list via the bulk payment API (500 recipients)
 * 5. Create a proposal to approve the payment list
 * 6. Approve the payment list proposal
 * 7. Verify all recipients received their tokens
 * 
 * Configuration:
 * - SANDBOX_RPC_URL: URL of the NEAR sandbox RPC (default: http://localhost:3030)
 * - API_URL: URL of the bulk payment API (default: http://localhost:8080)
 * - DAO_FACTORY_ID: Sputnik DAO factory account (default: sputnik-dao.near)
 * - BULK_PAYMENT_CONTRACT_ID: Bulk payment contract account
 * 
 * Usage:
 * - Docker: npm run test:docker
 * - Fly.io: SANDBOX_RPC_URL=https://your-app.fly.dev:3030 API_URL=https://your-app.fly.dev:8080 npm run test:fly
 */

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import * as nearAPI from 'near-api-js';
const { connect, keyStores, KeyPair, utils } = nearAPI;

// ============================================================================
// Configuration
// ============================================================================

// Storage cost calculation constants (matching bulk payment contract)
const BYTES_PER_RECORD = 216n; // AccountId (100) + amount (16) + status (~50) + overhead (~50)
const STORAGE_COST_PER_BYTE = 10n ** 19n; // yoctoNEAR per byte
const STORAGE_MARKUP_PERCENT = 110n; // 10% markup (110/100)

const CONFIG = {
  // URLs - configurable via environment variables
  SANDBOX_RPC_URL: process.env.SANDBOX_RPC_URL || 'http://localhost:3030',
  API_URL: process.env.API_URL || 'http://localhost:8080',
  
  // Contract IDs
  DAO_FACTORY_ID: process.env.DAO_FACTORY_ID || 'sputnik-dao.near',
  BULK_PAYMENT_CONTRACT_ID: process.env.BULK_PAYMENT_CONTRACT_ID || 'bulk-payment.sandbox',
  
  // Test parameters
  NUM_RECIPIENTS: parseInt(process.env.NUM_RECIPIENTS || '500', 10),
  PAYMENT_AMOUNT: process.env.PAYMENT_AMOUNT || '100000000000000000000000', // 0.1 NEAR per recipient
  
  // Genesis account credentials (default sandbox genesis account from near-sandbox-rs - PUBLIC TEST KEY)
  // See: https://github.com/near/near-sandbox-rs/blob/main/src/config.rs
  // This is the well-known sandbox test account key, safe for testing purposes only
  GENESIS_ACCOUNT_ID: process.env.GENESIS_ACCOUNT_ID || 'sandbox',
  GENESIS_PRIVATE_KEY: process.env.GENESIS_PRIVATE_KEY || 'ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB',
};

// ============================================================================
// Utilities
// ============================================================================

/**
 * Parse NEAR amount to yoctoNEAR
 */
function parseNEAR(amount) {
  return utils.format.parseNearAmount(amount.toString());
}

/**
 * Format yoctoNEAR to NEAR
 */
function formatNEAR(yoctoNear) {
  return utils.format.formatNearAmount(yoctoNear, 4);
}

/**
 * Generate an implicit account ID (64 character hex string)
 */
function generateImplicitAccountId(index) {
  // Generate a deterministic hex string based on index
  const hex = index.toString(16).padStart(8, '0');
  return hex.repeat(8); // 64 characters
}

/**
 * Generate a valid list_id (64-char hex-encoded SHA-256 hash)
 * The contract validates that list_id is exactly 64 hex characters [0-9a-f]
 */
function generateListId(submitterId, tokenId, payments) {
  const data = JSON.stringify({ submitter: submitterId, token_id: tokenId, payments });
  return createHash('sha256').update(data).digest('hex');
}

/**
 * Sleep for specified milliseconds
 */
function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

/**
 * Make HTTP request to the bulk payment API
 * @param {string} endpoint - API endpoint
 * @param {string} method - HTTP method
 * @param {object} body - Request body
 * @param {boolean} expectError - If true, don't throw on non-2xx responses
 */
async function apiRequest(endpoint, method = 'GET', body = null, expectError = false) {
  const url = `${CONFIG.API_URL}${endpoint}`;
  const options = {
    method,
    headers: {
      'Content-Type': 'application/json',
    },
  };
  
  if (body) {
    options.body = JSON.stringify(body);
  }
  
  const response = await fetch(url, options);
  
  if (!response.ok && !expectError) {
    const errorText = await response.text().catch(() => 'Unknown error');
    throw new Error(`API request failed: ${response.status} ${response.statusText} - ${errorText}`);
  }
  
  return response.json();
}

// ============================================================================
// NEAR Connection Setup
// ============================================================================

async function setupNearConnection() {
  const keyStore = new keyStores.InMemoryKeyStore();
  
  // Add genesis account key
  const keyPair = KeyPair.fromString(CONFIG.GENESIS_PRIVATE_KEY);
  await keyStore.setKey('sandbox', CONFIG.GENESIS_ACCOUNT_ID, keyPair);
  
  const connectionConfig = {
    networkId: 'sandbox',
    keyStore,
    nodeUrl: CONFIG.SANDBOX_RPC_URL,
  };
  
  const near = await connect(connectionConfig);
  const account = await near.account(CONFIG.GENESIS_ACCOUNT_ID);
  
  return { near, account, keyStore };
}

// ============================================================================
// DAO Operations
// ============================================================================

/**
 * Create a Sputnik DAO instance
 */
async function createDAO(account, daoName, creatorAccountId) {
  console.log(`\n📋 Creating DAO: ${daoName}.${CONFIG.DAO_FACTORY_ID}`);
  
  const createDaoArgs = {
    name: daoName,
    args: Buffer.from(JSON.stringify({
      config: {
        name: daoName,
        purpose: 'Testing bulk payments',
        metadata: '',
      },
      policy: {
        roles: [
          {
            kind: { Group: [creatorAccountId] },
            name: 'council',
            permissions: ['*:*'],
            vote_policy: {},
          },
        ],
        default_vote_policy: {
          weight_kind: 'RoleWeight',
          quorum: '0',
          threshold: [1, 2],
        },
        proposal_bond: '100000000000000000000000', // 0.1 NEAR
        proposal_period: '604800000000000', // 1 week in nanoseconds
        bounty_bond: '100000000000000000000000',
        bounty_forgiveness_period: '604800000000000',
      },
    })).toString('base64'),
  };
  
  const result = await account.functionCall({
    contractId: CONFIG.DAO_FACTORY_ID,
    methodName: 'create',
    args: createDaoArgs,
    gas: '300000000000000', // 300 TGas
    attachedDeposit: parseNEAR('100'), // 100 NEAR for DAO creation (needs funds for proposals)
  });
  
  console.log(`✅ DAO created: ${daoName}.${CONFIG.DAO_FACTORY_ID}`);
  return `${daoName}.${CONFIG.DAO_FACTORY_ID}`;
}

/**
 * Create a function call proposal in the DAO
 */
async function createProposal(account, daoAccountId, description, receiverId, methodName, args, deposit) {
  console.log(`\n📝 Creating proposal: ${description}`);
  
  const proposalArgs = {
    proposal: {
      description,
      kind: {
        FunctionCall: {
          receiver_id: receiverId,
          actions: [
            {
              method_name: methodName,
              args: Buffer.from(JSON.stringify(args)).toString('base64'),
              deposit: deposit || '0',
              gas: '150000000000000', // 150 TGas
            },
          ],
        },
      },
    },
  };
  
  const result = await account.functionCall({
    contractId: daoAccountId,
    methodName: 'add_proposal',
    args: proposalArgs,
    gas: '300000000000000',
    attachedDeposit: parseNEAR('0.1'), // Proposal bond
  });
  
  // Extract proposal ID from logs
  const logs = result.receipts_outcome
    .flatMap(o => o.outcome.logs)
    .join('\n');
  
  // Proposal ID is typically logged or we can query it
  // For simplicity, assume proposals are sequential starting from 0
  const proposalId = await getLastProposalId(account, daoAccountId);
  
  console.log(`✅ Proposal created with ID: ${proposalId}`);
  return proposalId;
}

/**
 * Get the last proposal ID from the DAO
 */
async function getLastProposalId(account, daoAccountId) {
  const result = await account.viewFunction({
    contractId: daoAccountId,
    methodName: 'get_last_proposal_id',
    args: {},
  });
  return result - 1; // get_last_proposal_id returns the next ID, so subtract 1
}

/**
 * Vote to approve a proposal
 */
async function approveProposal(account, daoAccountId, proposalId) {
  console.log(`\n✅ Approving proposal ${proposalId}`);
  
  await account.functionCall({
    contractId: daoAccountId,
    methodName: 'act_proposal',
    args: {
      id: proposalId,
      action: 'VoteApprove',
    },
    gas: '300000000000000',
  });
  
  console.log(`✅ Proposal ${proposalId} approved`);
}

/**
 * Get proposal status
 */
async function getProposalStatus(account, daoAccountId, proposalId) {
  const proposal = await account.viewFunction({
    contractId: daoAccountId,
    methodName: 'get_proposal',
    args: { id: proposalId },
  });
  return proposal.status;
}

// ============================================================================
// Bulk Payment Contract Operations
// ============================================================================

/**
 * Calculate storage cost for payment records
 */
function calculateStorageCost(numRecords) {
  // Uses constants defined at top of file
  const storageBytes = BYTES_PER_RECORD * BigInt(numRecords);
  const storageCost = storageBytes * STORAGE_COST_PER_BYTE;
  const totalCost = (storageCost * STORAGE_MARKUP_PERCENT) / 100n;
  return totalCost.toString();
}

/**
 * View payment list status
 */
async function viewPaymentList(account, listId) {
  const list = await account.viewFunction({
    contractId: CONFIG.BULK_PAYMENT_CONTRACT_ID,
    methodName: 'view_list',
    args: { list_id: listId },
  });
  return list;
}

/**
 * Check recipient balances
 */
async function checkRecipientBalances(near, recipients) {
  console.log('\n🔍 Checking recipient balances...');
  
  const balances = [];
  for (const recipient of recipients) {
    try {
      const account = await near.account(recipient.recipient);
      const balance = await account.getAccountBalance();
      balances.push({
        recipient: recipient.recipient,
        expected: recipient.amount,
        actual: balance.total,
        received: BigInt(balance.total) >= BigInt(recipient.amount),
      });
    } catch (e) {
      // Account may not exist yet for implicit accounts
      balances.push({
        recipient: recipient.recipient,
        expected: recipient.amount,
        actual: '0',
        received: false,
      });
    }
  }
  
  return balances;
}

// ============================================================================
// Main Test Flow (Top-Level Await)
// ============================================================================

try {
  console.log('🚀 Starting DAO Bulk Payment E2E Test');
  console.log('=====================================');
  console.log(`Sandbox RPC: ${CONFIG.SANDBOX_RPC_URL}`);
  console.log(`API URL: ${CONFIG.API_URL}`);
  console.log(`DAO Factory: ${CONFIG.DAO_FACTORY_ID}`);
  console.log(`Bulk Payment Contract: ${CONFIG.BULK_PAYMENT_CONTRACT_ID}`);
  console.log(`Number of Recipients: ${CONFIG.NUM_RECIPIENTS}`);
  console.log('=====================================\n');

// Step 1: Setup NEAR connection
console.log('📡 Connecting to NEAR sandbox...');
const { near, account, keyStore } = await setupNearConnection();
console.log(`✅ Connected as: ${account.accountId}`);

// Step 2: Check API health
console.log('\n🏥 Checking API health...');
const health = await apiRequest('/health');
assert.equal(health.status, 'healthy', 'API must be healthy');
console.log(`✅ API is healthy: ${JSON.stringify(health)}`);

// Step 3: Create DAO
const daoName = 'testdao';
const daoAccountId = await createDAO(account, daoName, account.accountId);

// Add DAO key to keystore (uses same key as genesis for testing)
const keyPair = KeyPair.fromString(CONFIG.GENESIS_PRIVATE_KEY);
await keyStore.setKey('sandbox', daoAccountId, keyPair);
const daoAccount = await near.account(daoAccountId);

// Step 4: Create proposal to buy_storage
const storageCost = calculateStorageCost(CONFIG.NUM_RECIPIENTS);
console.log(`\n💰 Storage cost for ${CONFIG.NUM_RECIPIENTS} records: ${formatNEAR(storageCost)} NEAR`);

const buyStorageProposalId = await createProposal(
  account,
  daoAccountId,
  `Buy storage for ${CONFIG.NUM_RECIPIENTS} payment records`,
  CONFIG.BULK_PAYMENT_CONTRACT_ID,
  'buy_storage',
  { num_records: CONFIG.NUM_RECIPIENTS },
  storageCost
);

// Step 5: Approve buy_storage proposal
await approveProposal(account, daoAccountId, buyStorageProposalId);

// Wait for execution
await sleep(2000);

// Step 6: Generate payment list
console.log(`\n📋 Generating payment list with ${CONFIG.NUM_RECIPIENTS} recipients...`);
const payments = [];
let totalPaymentAmount = BigInt(0);

for (let i = 0; i < CONFIG.NUM_RECIPIENTS; i++) {
  const recipient = generateImplicitAccountId(i);
  payments.push({
    recipient,
    amount: CONFIG.PAYMENT_AMOUNT,
  });
  totalPaymentAmount += BigInt(CONFIG.PAYMENT_AMOUNT);
}

console.log(`✅ Generated ${payments.length} payments`);
console.log(`💰 Total payment amount: ${formatNEAR(totalPaymentAmount.toString())} NEAR`);

// Step 7: Generate list_id (64-char hex SHA-256 hash)
const listId = generateListId(daoAccountId, 'native', payments);
console.log(`\n🔑 Generated list_id: ${listId}`);
assert.equal(listId.length, 64, 'list_id must be 64 characters');
assert.match(listId, /^[0-9a-f]{64}$/, 'list_id must be hex-encoded');

// Step 7b: Verify API rejects submission WITHOUT a DAO proposal
console.log('\n🔒 Testing API rejection without DAO proposal...');
const rejectResponse = await apiRequest('/submit-list', 'POST', {
  list_id: listId,
  submitter_id: daoAccountId,
  dao_contract_id: daoAccountId,
  token_id: 'native',
  payments,
}, true); // expectError = true

assert.equal(rejectResponse.success, false, 'Submit without DAO proposal must fail');
assert.ok(rejectResponse.error.includes('No pending DAO proposal found'), 
  `Error should mention missing DAO proposal: ${rejectResponse.error}`);
console.log(`✅ API correctly rejected submission: ${rejectResponse.error}`);

// Step 8: Create DAO proposal with list_id BEFORE submitting to API
// This is a security requirement - the API will verify this proposal exists
console.log('\n📝 Creating DAO proposal with list_id before API submission...');
const submitListProposalId = await createProposal(
  account,
  daoAccountId,
  `Bulk payment list: ${listId}`, // Include list_id in description for verification
  CONFIG.BULK_PAYMENT_CONTRACT_ID,
  'approve_list', // The approval method that will eventually be called
  { list_id: listId },
  totalPaymentAmount.toString()
);

// Step 9: Submit payment list via API (requires DAO proposal to exist)
console.log('\n📤 Submitting payment list via API...');
const submitResponse = await apiRequest('/submit-list', 'POST', {
  list_id: listId,
  submitter_id: daoAccountId,
  dao_contract_id: daoAccountId,
  token_id: 'native',
  payments,
});

assert.equal(submitResponse.success, true, `Submit must succeed: ${submitResponse.error}`);
assert.equal(submitResponse.list_id, listId, 'Returned list_id must match submitted');
console.log(`✅ Payment list submitted with ID: ${listId}`);

// Step 10: Approve the payment list proposal (already created in Step 8)
await approveProposal(account, daoAccountId, submitListProposalId);

// Wait for execution
await sleep(2000);

// Step 11: Verify list is approved
console.log('\n🔍 Verifying payment list status...');
const listStatus = await viewPaymentList(account, listId);
console.log(`📊 List status: ${listStatus.status}`);
console.log(`📊 Total payments: ${listStatus.payments.length}`);

assert.equal(listStatus.status, 'Approved', `Payment list must be Approved, got: ${listStatus.status}`);
assert.equal(listStatus.payments.length, CONFIG.NUM_RECIPIENTS, `Must have ${CONFIG.NUM_RECIPIENTS} payments`);

// Step 12: Wait for payout processing (background worker processes approved lists)
console.log('\n⏳ Waiting for payout processing...');
let allPaid = false;
let attempts = 0;
const maxAttempts = 60; // 5 minutes at 5-second intervals

while (!allPaid && attempts < maxAttempts) {
  await sleep(5000);
  attempts++;
  
  const currentStatus = await apiRequest(`/list/${listId}`);
  assert.equal(currentStatus.success, true, `Must be able to get list status: ${currentStatus.error}`);
  
  const { list } = currentStatus;
  const progress = ((list.paid_payments / list.total_payments) * 100).toFixed(1);
  console.log(`📊 Progress: ${list.paid_payments}/${list.total_payments} (${progress}%)`);
  
  if (list.pending_payments === 0 && list.failed_payments === 0) {
    allPaid = true;
  }
}

assert.equal(allPaid, true, 'All payments must complete within timeout');

// Step 13: Verify recipient balances
console.log('\n🔍 Verifying recipient balances...');
const sampleRecipients = payments.slice(0, 10); // Check first 10 recipients
const balances = await checkRecipientBalances(near, sampleRecipients);

let successCount = 0;
for (const balance of balances) {
  if (balance.received) {
    successCount++;
    console.log(`✅ ${balance.recipient.substring(0, 16)}...: ${formatNEAR(balance.actual)} NEAR`);
  } else {
    console.log(`❌ ${balance.recipient.substring(0, 16)}...: ${formatNEAR(balance.actual)} NEAR (expected ${formatNEAR(balance.expected)})`);
  }
}

// Step 14: Final verification
console.log('\n=====================================');
console.log('📊 Test Summary');
console.log('=====================================');
console.log(`DAO Created: ${daoAccountId}`);
console.log(`Payment List ID: ${listId}`);
console.log(`Total Recipients: ${CONFIG.NUM_RECIPIENTS}`);
console.log(`Sample Recipients Verified: ${successCount}/${sampleRecipients.length}`);

const finalStatus = await viewPaymentList(account, listId);
const paidCount = finalStatus.payments.filter(p => p.status === 'Paid').length;
const pendingCount = finalStatus.payments.filter(p => p.status === 'Pending').length;
const failedCount = finalStatus.payments.filter(p => p.status && p.status.Failed).length;

console.log(`Paid: ${paidCount}`);
console.log(`Pending: ${pendingCount}`);
console.log(`Failed: ${failedCount}`);
console.log('=====================================\n');

// Hard assertions
assert.equal(paidCount, CONFIG.NUM_RECIPIENTS, `All ${CONFIG.NUM_RECIPIENTS} payments must be Paid`);
assert.equal(pendingCount, 0, 'No payments should be Pending');
assert.equal(failedCount, 0, 'No payments should be Failed');
assert.equal(successCount, sampleRecipients.length, 'All sample recipients must have received their tokens');

console.log('🎉 Test PASSED: All payments completed successfully!');
process.exit(0);

} catch (error) {
  console.error('❌ Test FAILED:', error.message);
  if (error.stack) {
    console.error(error.stack);
  }
  process.exit(1);
}
