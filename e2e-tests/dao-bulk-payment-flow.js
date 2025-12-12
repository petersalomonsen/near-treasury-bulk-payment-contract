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
import { NearRpcClient, block as rpcBlock, chunk as rpcChunk, tx as rpcTx } from '@near-js/jsonrpc-client';
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
  BULK_PAYMENT_CONTRACT_ID: process.env.BULK_PAYMENT_CONTRACT_ID || 'bulk-payment.near',
  
  // Test parameters
  NUM_RECIPIENTS: parseInt(process.env.NUM_RECIPIENTS || '500', 10),
  PAYMENT_AMOUNT: process.env.PAYMENT_AMOUNT || '100000000000000000000000', // 0.1 NEAR per recipient
  
  // Genesis account credentials (default sandbox genesis account from near-sandbox-rs - PUBLIC TEST KEY)
  // See: https://github.com/near/near-sandbox-rs/blob/main/src/config.rs
  // This is the well-known sandbox test account key, safe for testing purposes only
  GENESIS_ACCOUNT_ID: process.env.GENESIS_ACCOUNT_ID || 'test.near',
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
 * The API validates that list_id matches SHA-256(canonical_json(sorted_payments))
 * This ensures the payload matches the hash (integrity guarantee)
 * 
 * IMPORTANT: The hash must match the Rust API's serde_json serialization which:
 * 1. Sorts object keys alphabetically
 * 2. Sorts payments by recipient
 */
function generateListId(submitterId, tokenId, payments) {
  // Sort payments by recipient for deterministic ordering (must match API)
  const sortedPayments = [...payments].sort((a, b) => a.recipient.localeCompare(b.recipient));
  
  // Create canonical JSON with alphabetically sorted keys (matches Rust serde_json)
  // Key order: payments, submitter, token_id (alphabetical)
  // Payment key order: amount, recipient (alphabetical)
  const canonical = JSON.stringify({
    payments: sortedPayments.map(p => ({ amount: p.amount, recipient: p.recipient })),
    submitter: submitterId,
    token_id: tokenId,
  });
  
  return createHash('sha256').update(canonical).digest('hex');
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
  
  const daoAccountId = `${daoName}.${CONFIG.DAO_FACTORY_ID}`;
  
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
  
  try {
    const result = await account.functionCall({
      contractId: CONFIG.DAO_FACTORY_ID,
      methodName: 'create',
      args: createDaoArgs,
      gas: '300000000000000', // 300 TGas
      attachedDeposit: parseNEAR('100'), // 100 NEAR for DAO creation (needs funds for proposals)
    });
    
    console.log(`✅ DAO created: ${daoAccountId}`);
  } catch (error) {
    if (error.message && error.message.includes('already exists')) {
      console.log(`ℹ️  DAO already exists: ${daoAccountId} (reusing)`);
    } else {
      throw error;
    }
  }
  
  return daoAccountId;
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

// Check DAO balance and top up if needed
const daoState = await daoAccount.state();
const daoBalance = BigInt(daoState.amount);
const minBalance = parseNEAR('100'); // Need at least 100 NEAR for operations
console.log(`\n💼 DAO balance: ${formatNEAR(daoBalance.toString())} NEAR`);

if (daoBalance < BigInt(minBalance)) {
  const topUpAmount = parseNEAR('200'); // Top up with 200 NEAR
  console.log(`📤 Topping up DAO with ${formatNEAR(topUpAmount)} NEAR...`);
  await account.sendMoney(daoAccountId, BigInt(topUpAmount));
  console.log(`✅ DAO topped up`);
}

// Step 4: Check existing storage credits and buy more if needed
const storageCost = calculateStorageCost(CONFIG.NUM_RECIPIENTS);
console.log(`\n💰 Storage cost for ${CONFIG.NUM_RECIPIENTS} records: ${formatNEAR(storageCost)} NEAR`);

// Check existing storage credits
let existingCredits = BigInt(0);
try {
  const credits = await account.viewFunction({
    contractId: CONFIG.BULK_PAYMENT_CONTRACT_ID,
    methodName: 'view_storage_credits',
    args: { account_id: daoAccountId },
  });
  existingCredits = BigInt(credits || '0');
  console.log(`📊 Existing storage credits: ${formatNEAR(existingCredits.toString())} NEAR`);
} catch (e) {
  console.log(`📊 No existing storage credits found`);
}

const storageCostBigInt = BigInt(storageCost);
if (existingCredits >= storageCostBigInt) {
  console.log(`✅ Sufficient storage credits available, skipping buy_storage`);
} else {
  const additionalNeeded = storageCostBigInt - existingCredits;
  console.log(`📝 Need to buy additional storage: ${formatNEAR(additionalNeeded.toString())} NEAR`);
  
  const buyStorageProposalId = await createProposal(
    account,
    daoAccountId,
    `Buy storage for ${CONFIG.NUM_RECIPIENTS} payment records`,
    CONFIG.BULK_PAYMENT_CONTRACT_ID,
    'buy_storage',
    { num_records: CONFIG.NUM_RECIPIENTS },
    storageCost // Buy full amount (contract handles credits)
  );

  // Step 5: Approve buy_storage proposal
  await approveProposal(account, daoAccountId, buyStorageProposalId);

  // Wait for execution
  await sleep(2000);
}

// Step 6: Generate payment list with unique amounts for each run
console.log(`\n📋 Generating payment list with ${CONFIG.NUM_RECIPIENTS} recipients...`);
const testRunNonce = Date.now(); // Make each test run unique
const payments = [];
let totalPaymentAmount = BigInt(0);

for (let i = 0; i < CONFIG.NUM_RECIPIENTS; i++) {
  const recipient = generateImplicitAccountId(i);
  // Add small random variation to make list_id unique per run
  // Use timestamp mod 1000000 to add a unique offset to each run
  const baseAmount = BigInt(CONFIG.PAYMENT_AMOUNT);
  const variation = BigInt((testRunNonce % 1000000) + i); // Unique per run + recipient
  const uniqueAmount = (baseAmount + variation).toString();
  payments.push({
    recipient,
    amount: uniqueAmount,
  });
  totalPaymentAmount += BigInt(uniqueAmount);
}

console.log(`✅ Generated ${payments.length} payments`);
console.log(`💰 Total payment amount: ${formatNEAR(totalPaymentAmount.toString())} NEAR`);

// Step 7: Generate list_id (64-char hex SHA-256 hash)
const listId = generateListId(daoAccountId, 'native', payments);
console.log(`\n🔑 Generated list_id: ${listId}`);
assert.equal(listId.length, 64, 'list_id must be 64 characters');
assert.match(listId, /^[0-9a-f]{64}$/, 'list_id must be hex-encoded');

// Step 7b: Verify API rejects submission with WRONG hash (payload doesn't match list_id)
console.log('\n🔒 Testing API rejection with mismatched hash...');
const wrongHashResponse = await apiRequest('/submit-list', 'POST', {
  list_id: listId,
  submitter_id: daoAccountId,
  dao_contract_id: daoAccountId,
  token_id: 'native',
  // Tamper with payments - change first recipient's amount
  payments: payments.map((p, i) => i === 0 ? { ...p, amount: '999' } : p),
}, true); // expectError = true

assert.equal(wrongHashResponse.success, false, 'Submit with wrong hash must fail');
assert.ok(wrongHashResponse.error.includes('does not match computed hash'), 
  `Error should mention hash mismatch: ${wrongHashResponse.error}`);
console.log(`✅ API correctly rejected tampered payload: ${wrongHashResponse.error}`);

// Step 7c: Verify API rejects submission WITHOUT a DAO proposal
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
let allProcessed = false;
let attempts = 0;
const maxAttempts = 60; // 5 minutes at 5-second intervals

while (!allProcessed && attempts < maxAttempts) {
  await sleep(5000);
  attempts++;
  
  const currentStatus = await apiRequest(`/list/${listId}`);
  assert.equal(currentStatus.success, true, `Must be able to get list status: ${currentStatus.error}`);
  
  const { list } = currentStatus;
  const progress = ((list.processed_payments / list.total_payments) * 100).toFixed(1);
  console.log(`📊 Progress: ${list.processed_payments}/${list.total_payments} (${progress}%)`);
  
  // All payments are complete when there are no pending payments
  if (list.pending_payments === 0) {
    allProcessed = true;
  }
}

assert.equal(allProcessed, true, 'All payments must complete within timeout');

// Step 13: Verify all payments have block_height registered
console.log('\n🔍 Verifying all payments have block_height...');
const finalStatus = await viewPaymentList(account, listId);

// Check that every payment has a block_height (status is {Paid: {block_height: N}})
const paymentsWithBlockHeight = finalStatus.payments.filter(p => 
  p.status && p.status.Paid && typeof p.status.Paid.block_height === 'number'
);
const paymentsWithoutBlockHeight = finalStatus.payments.filter(p => 
  !p.status || !p.status.Paid || typeof p.status.Paid.block_height !== 'number'
);

console.log(`📊 Payments with block_height: ${paymentsWithBlockHeight.length}/${finalStatus.payments.length}`);

if (paymentsWithoutBlockHeight.length > 0) {
  console.log(`❌ Payments without block_height:`);
  paymentsWithoutBlockHeight.slice(0, 5).forEach(p => {
    console.log(`   - ${p.recipient}: status = ${JSON.stringify(p.status)}`);
  });
}

assert.equal(
  paymentsWithBlockHeight.length, 
  CONFIG.NUM_RECIPIENTS, 
  `All ${CONFIG.NUM_RECIPIENTS} payments must have block_height registered`
);
console.log(`✅ All payments have block_height registered`);

// Step 14: Verify payment transactions exist in blocks (sample verification)
console.log('\n🔗 Verifying payment transactions in blocks...');

// Create JSON-RPC client for direct RPC calls
const rpcClient = new NearRpcClient({ endpoint: CONFIG.SANDBOX_RPC_URL });

// Group payments by block_height to minimize RPC calls
const paymentsByBlock = new Map();
for (const payment of finalStatus.payments) {
  const blockHeight = payment.status.Paid.block_height;
  if (!paymentsByBlock.has(blockHeight)) {
    paymentsByBlock.set(blockHeight, []);
  }
  paymentsByBlock.get(blockHeight).push(payment);
}

console.log(`📊 Payments distributed across ${paymentsByBlock.size} blocks`);

// Verify a sample of blocks (up to 5 blocks)
const blockHeights = Array.from(paymentsByBlock.keys()).slice(0, 5);
let verifiedTransactions = 0;
let failedReceipts = [];
let transactionErrors = [];

for (const blockHeight of blockHeights) {
  const blockPayments = paymentsByBlock.get(blockHeight);
  console.log(`\n📦 Checking block ${blockHeight} (${blockPayments.length} payments)...`);
  
  // Get the block using JSON-RPC client
  const blockData = await rpcBlock(rpcClient, { blockId: Number(blockHeight) });
  
  // Get all chunks in the block
  const chunkHashes = blockData.chunks.map(c => c.chunkHash);
  
  // Check each chunk for transactions from the bulk payment contract
  let foundPayoutTx = false;
  for (const currentChunkHash of chunkHashes) {
    // Get chunk using JSON-RPC client
    const chunkData = await rpcChunk(rpcClient, { chunkId: currentChunkHash });
    
    // Look for transactions calling payout_batch on the bulk payment contract
    const payoutTxs = (chunkData.transactions || []).filter(tx => 
      tx.receiverId === CONFIG.BULK_PAYMENT_CONTRACT_ID
    );
    
    if (payoutTxs.length > 0) {
      foundPayoutTx = true;
      console.log(`   ✅ Found ${payoutTxs.length} transaction(s) to bulk payment contract in chunk ${currentChunkHash.substring(0, 16)}...`);
      
      // Verify transaction outcomes (receipts) for successful execution
      for (const tx of payoutTxs) {
        const txHash = tx.hash;
        const senderAccountId = tx.signerId;
        
        // Get transaction status using JSON-RPC client
        const txStatus = await rpcTx(rpcClient, { txHash, senderAccountId });
        
        // Check for any failed receipts
        const txFailedReceipts = txStatus.receiptsOutcome.filter(
          ro => ro.outcome.status && ro.outcome.status.Failure
        );
        
        if (txFailedReceipts.length > 0) {
          console.log(`   ❌ Transaction ${txHash.substring(0, 16)}... has ${txFailedReceipts.length} failed receipt(s)`);
          txFailedReceipts.forEach(fr => {
            console.log(`      Failure: ${JSON.stringify(fr.outcome.status.Failure)}`);
            failedReceipts.push({
              txHash,
              blockHeight,
              failure: fr.outcome.status.Failure
            });
          });
        } else {
          console.log(`   ✅ Transaction ${txHash.substring(0, 16)}... succeeded with ${txStatus.receiptsOutcome.length} receipt(s)`);
          verifiedTransactions++;
        }
      }
    }
  }
  
  if (!foundPayoutTx) {
    const error = `No transactions to bulk payment contract found in block ${blockHeight}`;
    console.log(`   ❌ ${error}`);
    transactionErrors.push({ blockHeight, error });
  }
}

console.log(`\n📊 Transaction verification summary:`);
console.log(`   Verified transactions: ${verifiedTransactions}`);
console.log(`   Failed receipts: ${failedReceipts.length}`);
console.log(`   Transaction errors: ${transactionErrors.length}`);

// Hard assertions for transaction verification
assert.equal(failedReceipts.length, 0, `Found ${failedReceipts.length} failed receipt(s): ${JSON.stringify(failedReceipts)}`);
assert.equal(transactionErrors.length, 0, `Found ${transactionErrors.length} transaction error(s): ${JSON.stringify(transactionErrors)}`);
assert.ok(verifiedTransactions > 0, 'Must have at least one verified transaction');

console.log(`✅ All ${verifiedTransactions} transaction(s) in ${blockHeights.length} sample block(s) verified successfully`);

// Step 15: Verify recipient balances
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

// Step 16: Final verification
console.log('\n=====================================');
console.log('📊 Test Summary');
console.log('=====================================');
console.log(`DAO Created: ${daoAccountId}`);
console.log(`Payment List ID: ${listId}`);
console.log(`Total Recipients: ${CONFIG.NUM_RECIPIENTS}`);
console.log(`Payments with block_height: ${paymentsWithBlockHeight.length}`);
console.log(`Unique blocks used: ${paymentsByBlock.size}`);
console.log(`Sample blocks verified: ${blockHeights.length}`);
console.log(`Sample recipients verified: ${successCount}/${sampleRecipients.length}`);
console.log('=====================================\n');

// Hard assertions
assert.equal(paymentsWithBlockHeight.length, CONFIG.NUM_RECIPIENTS, `All ${CONFIG.NUM_RECIPIENTS} payments must have block_height`);
assert.equal(successCount, sampleRecipients.length, 'All sample recipients must have received their tokens');

console.log('🎉 Test PASSED: All payments completed successfully with block_height tracking!');
process.exit(0);

} catch (error) {
  console.error('❌ Test FAILED:', error.message);
  if (error.stack) {
    console.error(error.stack);
  }
  process.exit(1);
}
