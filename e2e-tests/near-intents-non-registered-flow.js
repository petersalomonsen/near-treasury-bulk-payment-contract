/**
 * End-to-End Test: NEAR Intents Token Payments to Non-Registered Accounts
 * 
 * This test demonstrates the behavior when making bulk payments with NEAR Intents tokens
 * (wrap.near on intents.near) to accounts that are not registered with the token contract.
 * 
 * Test Scenario:
 * 1. Use existing DAO from dao-bulk-payment-flow.js (testdao.sputnik-dao.near)
 * 2. Create a payment list with nep141:wrap.near tokens (NEAR Intents format)
 * 3. Mix of registered and non-registered recipients:
 *    - Some implicit accounts will be registered with intents.near before payment
 *    - Some implicit accounts will NOT be registered (non-registered)
 * 4. Submit and approve the payment list
 * 5. Process payments
 * 6. Verify ALL payments are marked as processed (have block_height)
 * 7. Verify registered accounts show balance changes and successful transactions
 * 8. Verify non-registered accounts show failed receipts but still have block_height
 * 
 * Configuration:
 * - SANDBOX_RPC_URL: URL of the NEAR sandbox RPC (default: http://localhost:3030)
 * - API_URL: URL of the bulk payment API (default: http://localhost:8080)
 * - BULK_PAYMENT_CONTRACT_ID: Bulk payment contract account
 * 
 * Prerequisites:
 * - dao-bulk-payment-flow.js must have been run first to create the DAO
 * - intents.near contract must be deployed in the sandbox
 * - wrap.near contract must be deployed in the sandbox
 */

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import * as nearAPI from 'near-api-js';
import { NearRpcClient, tx as rpcTx } from '@near-js/jsonrpc-client';
const { connect, keyStores, KeyPair, utils } = nearAPI;

// ============================================================================
// Configuration
// ============================================================================

const CONFIG = {
  // URLs - configurable via environment variables
  SANDBOX_RPC_URL: process.env.SANDBOX_RPC_URL || 'http://localhost:3030',
  API_URL: process.env.API_URL || 'http://localhost:8080',
  
  // Contract IDs
  DAO_ACCOUNT_ID: process.env.DAO_ACCOUNT_ID || 'testdao.sputnik-dao.near',
  BULK_PAYMENT_CONTRACT_ID: process.env.BULK_PAYMENT_CONTRACT_ID || 'bulk-payment.near',
  INTENTS_CONTRACT_ID: process.env.INTENTS_CONTRACT_ID || 'intents.near',
  WRAP_TOKEN_ID: process.env.WRAP_TOKEN_ID || 'wrap.near',
  
  // Test parameters
  NUM_REGISTERED: parseInt(process.env.NUM_REGISTERED || '3', 10),
  NUM_NON_REGISTERED: parseInt(process.env.NUM_NON_REGISTERED || '3', 10),
  PAYMENT_AMOUNT: process.env.PAYMENT_AMOUNT || '1000000000000000000000000', // 1 wNEAR
  
  // Genesis account credentials (sandbox test key)
  GENESIS_ACCOUNT_ID: process.env.GENESIS_ACCOUNT_ID || 'test.near',
  GENESIS_PRIVATE_KEY: process.env.GENESIS_PRIVATE_KEY || 'ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB',
};

// Storage cost calculation constants
const BYTES_PER_RECORD = 216n;
const STORAGE_COST_PER_BYTE = 10n ** 19n;
const STORAGE_MARKUP_PERCENT = 110n;

// ============================================================================
// Utilities
// ============================================================================

function parseNEAR(amount) {
  return utils.format.parseNearAmount(amount.toString());
}

function formatNEAR(yoctoNear) {
  return utils.format.formatNearAmount(yoctoNear, 4);
}

function generateImplicitAccountId(index) {
  const hex = index.toString(16).padStart(8, '0');
  return hex.repeat(8); // 64 characters
}

function generateListId(submitterId, tokenId, payments) {
  const sortedPayments = [...payments].sort((a, b) => a.recipient.localeCompare(b.recipient));
  const canonical = JSON.stringify({
    payments: sortedPayments.map(p => ({ amount: p.amount, recipient: p.recipient })),
    submitter: submitterId,
    token_id: tokenId,
  });
  return createHash('sha256').update(canonical).digest('hex');
}

function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

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

function calculateStorageCost(numRecords) {
  const storageBytes = BYTES_PER_RECORD * BigInt(numRecords);
  const storageCost = storageBytes * STORAGE_COST_PER_BYTE;
  const totalCost = (storageCost * STORAGE_MARKUP_PERCENT) / 100n;
  return totalCost.toString();
}

async function viewPaymentList(account, listId) {
  const list = await account.viewFunction({
    contractId: CONFIG.BULK_PAYMENT_CONTRACT_ID,
    methodName: 'view_list',
    args: { list_id: listId },
  });
  return list;
}

// ============================================================================
// NEAR Connection Setup
// ============================================================================

async function setupNearConnection() {
  const keyStore = new keyStores.InMemoryKeyStore();
  
  const keyPair = KeyPair.fromString(CONFIG.GENESIS_PRIVATE_KEY);
  await keyStore.setKey('sandbox', CONFIG.GENESIS_ACCOUNT_ID, keyPair);
  await keyStore.setKey('sandbox', CONFIG.DAO_ACCOUNT_ID, keyPair); // DAO uses same key in tests
  
  const connectionConfig = {
    networkId: 'sandbox',
    keyStore,
    nodeUrl: CONFIG.SANDBOX_RPC_URL,
  };
  
  const near = await connect(connectionConfig);
  const genesisAccount = await near.account(CONFIG.GENESIS_ACCOUNT_ID);
  const daoAccount = await near.account(CONFIG.DAO_ACCOUNT_ID);
  
  return { near, genesisAccount, daoAccount, keyStore };
}

// ============================================================================
// Multi-Token (NEAR Intents) Operations
// ============================================================================

/**
 * Register an account with intents.near multi-token contract
 */
async function registerWithIntents(account, accountToRegister) {
  console.log(`📝 Registering ${accountToRegister} with ${CONFIG.INTENTS_CONTRACT_ID}...`);
  
  try {
    await account.functionCall({
      contractId: CONFIG.INTENTS_CONTRACT_ID,
      methodName: 'mt_storage_deposit',
      args: {
        account_id: accountToRegister,
        registration_only: true,
      },
      gas: '30000000000000', // 30 TGas
      attachedDeposit: parseNEAR('0.00125'), // Standard storage deposit
    });
    console.log(`✅ Registered ${accountToRegister}`);
    return true;
  } catch (error) {
    if (error.message && error.message.includes('already registered')) {
      console.log(`ℹ️  ${accountToRegister} already registered`);
      return true;
    }
    throw error;
  }
}

/**
 * Get multi-token balance for an account
 */
async function getMultiTokenBalance(account, accountId, tokenId) {
  try {
    const balance = await account.viewFunction({
      contractId: CONFIG.INTENTS_CONTRACT_ID,
      methodName: 'mt_balance_of',
      args: { 
        account_id: accountId,
        token_id: tokenId,
      },
    });
    return balance;
  } catch (error) {
    return '0';
  }
}

/**
 * Deposit tokens into intents.near
 */
async function depositToIntents(account, tokenContractId, amount) {
  console.log(`💸 Depositing ${amount} tokens to intents.near...`);
  
  // First need to deposit wrap.near tokens to intents.near via ft_transfer_call
  await account.functionCall({
    contractId: tokenContractId,
    methodName: 'ft_transfer_call',
    args: {
      receiver_id: CONFIG.INTENTS_CONTRACT_ID,
      amount: amount,
      msg: '', // Empty message for simple deposit
    },
    gas: '100000000000000', // 100 TGas for cross-contract call
    attachedDeposit: '1', // 1 yoctoNEAR for security
  });
  
  console.log(`✅ Deposited ${amount} tokens to intents.near`);
}

// ============================================================================
// Main Test Flow
// ============================================================================

try {
  console.log('🚀 Starting NEAR Intents Non-Registered Account E2E Test');
  console.log('========================================================');
  console.log(`Sandbox RPC: ${CONFIG.SANDBOX_RPC_URL}`);
  console.log(`API URL: ${CONFIG.API_URL}`);
  console.log(`DAO Account: ${CONFIG.DAO_ACCOUNT_ID}`);
  console.log(`Bulk Payment Contract: ${CONFIG.BULK_PAYMENT_CONTRACT_ID}`);
  console.log(`Intents Contract: ${CONFIG.INTENTS_CONTRACT_ID}`);
  console.log(`Token: nep141:${CONFIG.WRAP_TOKEN_ID}`);
  console.log(`Registered Recipients: ${CONFIG.NUM_REGISTERED}`);
  console.log(`Non-Registered Recipients: ${CONFIG.NUM_NON_REGISTERED}`);
  console.log('========================================================\n');

// Step 1: Setup NEAR connection
console.log('📡 Connecting to NEAR sandbox...');
const { near, genesisAccount, daoAccount, keyStore } = await setupNearConnection();
console.log(`✅ Connected as genesis: ${genesisAccount.accountId}`);
console.log(`✅ Connected as DAO: ${daoAccount.accountId}`);

// Step 2: Check API health
console.log('\n🏥 Checking API health...');
const health = await apiRequest('/health');
assert.equal(health.status, 'healthy', 'API must be healthy');
console.log(`✅ API is healthy`);

// Step 3: Generate recipient accounts
console.log(`\n👥 Generating ${CONFIG.NUM_REGISTERED + CONFIG.NUM_NON_REGISTERED} recipient accounts...`);
const registeredRecipients = [];
const nonRegisteredRecipients = [];

// Use timestamp with offset to avoid collisions with other tests
// Offset: 10000000 to distinguish from fungible-token test (1000000) and dao test (0)
const startIndex = Date.now() + 10000000;

// Generate registered recipients
for (let i = 0; i < CONFIG.NUM_REGISTERED; i++) {
  const recipient = generateImplicitAccountId(startIndex + i);
  registeredRecipients.push(recipient);
}

// Generate non-registered recipients
for (let i = 0; i < CONFIG.NUM_NON_REGISTERED; i++) {
  const recipient = generateImplicitAccountId(startIndex + CONFIG.NUM_REGISTERED + i);
  nonRegisteredRecipients.push(recipient);
}

console.log(`✅ Generated ${registeredRecipients.length} registered recipients`);
console.log(`✅ Generated ${nonRegisteredRecipients.length} non-registered recipients`);

// Step 4: Register some accounts with intents.near
console.log('\n📝 Registering accounts with intents.near...');
for (const recipient of registeredRecipients) {
  await registerWithIntents(genesisAccount, recipient);
  await sleep(500); // Small delay between registrations
}
console.log(`✅ Registered ${registeredRecipients.length} accounts`);

// Step 5: Ensure DAO is registered with intents.near and has tokens
console.log('\n💰 Preparing DAO account...');
await registerWithIntents(genesisAccount, CONFIG.DAO_ACCOUNT_ID);

// Check DAO's multi-token balance for wrap.near
const tokenId = CONFIG.WRAP_TOKEN_ID; // In intents.near, tokens are referenced by their contract ID
let daoTokenBalance = await getMultiTokenBalance(daoAccount, CONFIG.DAO_ACCOUNT_ID, tokenId);
console.log(`📊 DAO ${tokenId} balance in intents.near: ${daoTokenBalance}`);

const totalRecipients = CONFIG.NUM_REGISTERED + CONFIG.NUM_NON_REGISTERED;
const totalPaymentAmount = BigInt(CONFIG.PAYMENT_AMOUNT) * BigInt(totalRecipients);
const requiredBalance = totalPaymentAmount * 2n; // 2x for safety

if (BigInt(daoTokenBalance) < requiredBalance) {
  const neededTokens = requiredBalance - BigInt(daoTokenBalance);
  console.log(`📤 DAO needs ${neededTokens.toString()} more tokens in intents.near`);
  
  // First, ensure wrap.near has enough tokens
  console.log(`💵 Getting wNEAR tokens...`);
  await genesisAccount.functionCall({
    contractId: CONFIG.WRAP_TOKEN_ID,
    methodName: 'near_deposit',
    args: {},
    gas: '30000000000000',
    attachedDeposit: (neededTokens + parseNEAR('1')).toString(), // Extra for fees
  });
  
  // Deposit tokens into intents.near for DAO
  await depositToIntents(genesisAccount, CONFIG.WRAP_TOKEN_ID, neededTokens.toString());
  
  // Now transfer the multi-tokens to DAO
  await genesisAccount.functionCall({
    contractId: CONFIG.INTENTS_CONTRACT_ID,
    methodName: 'mt_transfer',
    args: {
      receiver_id: CONFIG.DAO_ACCOUNT_ID,
      token_id: tokenId,
      amount: neededTokens.toString(),
    },
    gas: '30000000000000',
    attachedDeposit: '1',
  });
  
  daoTokenBalance = await getMultiTokenBalance(daoAccount, CONFIG.DAO_ACCOUNT_ID, tokenId);
  console.log(`✅ DAO token balance in intents.near now: ${daoTokenBalance}`);
}

// Step 6: Ensure bulk payment contract is registered with intents.near
console.log('\n📝 Ensuring bulk payment contract is registered with intents.near...');
await registerWithIntents(genesisAccount, CONFIG.BULK_PAYMENT_CONTRACT_ID);

// Step 7: Check and buy storage credits if needed
const storageCost = calculateStorageCost(totalRecipients);
console.log(`\n💰 Storage cost for ${totalRecipients} records: ${formatNEAR(storageCost)} NEAR`);

let existingCredits = BigInt(0);
try {
  const credits = await genesisAccount.viewFunction({
    contractId: CONFIG.BULK_PAYMENT_CONTRACT_ID,
    methodName: 'view_storage_credits',
    args: { account_id: CONFIG.DAO_ACCOUNT_ID },
  });
  existingCredits = BigInt(credits || '0');
  console.log(`📊 Existing storage credits: ${formatNEAR(existingCredits.toString())} NEAR`);
} catch (e) {
  console.log(`📊 No existing storage credits found`);
}

const storageCostBigInt = BigInt(storageCost);
if (existingCredits < storageCostBigInt) {
  const additionalNeeded = storageCostBigInt - existingCredits;
  console.log(`📝 Buying additional storage: ${formatNEAR(additionalNeeded.toString())} NEAR`);
  
  await daoAccount.functionCall({
    contractId: CONFIG.BULK_PAYMENT_CONTRACT_ID,
    methodName: 'buy_storage',
    args: { num_records: totalRecipients },
    gas: '30000000000000',
    attachedDeposit: storageCost,
  });
  
  console.log(`✅ Storage purchased`);
}

// Step 8: Generate payment list
console.log(`\n📋 Generating payment list...`);
const testRunNonce = Date.now();
const payments = [];

// Add registered recipients
for (let i = 0; i < registeredRecipients.length; i++) {
  const baseAmount = BigInt(CONFIG.PAYMENT_AMOUNT);
  const variation = BigInt((testRunNonce % 1000000) + i);
  payments.push({
    recipient: registeredRecipients[i],
    amount: (baseAmount + variation).toString(),
  });
}

// Add non-registered recipients
for (let i = 0; i < nonRegisteredRecipients.length; i++) {
  const baseAmount = BigInt(CONFIG.PAYMENT_AMOUNT);
  const variation = BigInt((testRunNonce % 1000000) + registeredRecipients.length + i);
  payments.push({
    recipient: nonRegisteredRecipients[i],
    amount: (baseAmount + variation).toString(),
  });
}

console.log(`✅ Generated ${payments.length} payments`);

// Step 9: Generate list_id and submit to contract
// Use nep141: prefix for NEAR Intents token format
const tokenIdForList = `nep141:${CONFIG.WRAP_TOKEN_ID}`;
const listId = generateListId(CONFIG.DAO_ACCOUNT_ID, tokenIdForList, payments);
console.log(`\n🔑 Generated list_id: ${listId}`);
console.log(`🔖 Token ID: ${tokenIdForList}`);

// Step 10: Submit payment list to contract
console.log('\n📤 Submitting payment list to contract...');
await daoAccount.functionCall({
  contractId: CONFIG.BULK_PAYMENT_CONTRACT_ID,
  methodName: 'submit_list',
  args: {
    token_id: tokenIdForList,
    payments: payments,
  },
  gas: '300000000000000',
});

console.log(`✅ Payment list submitted`);

// Step 11: Approve payment list via mt_transfer_call
console.log('\n✅ Approving payment list via mt_transfer_call...');

const totalAmount = payments.reduce((sum, p) => sum + BigInt(p.amount), 0n);
console.log(`💸 Transferring ${totalAmount.toString()} tokens to bulk payment contract...`);

// Use mt_transfer_call to approve the list (intents.near pattern)
// Note: We use the base token ID (wrap.near) here, not the nep141: prefix,
// because mt_transfer_call is a method on intents.near multi-token contract,
// which uses the underlying token contract ID as the token_id parameter.
await daoAccount.functionCall({
  contractId: CONFIG.INTENTS_CONTRACT_ID,
  methodName: 'mt_transfer_call',
  args: {
    receiver_id: CONFIG.BULK_PAYMENT_CONTRACT_ID,
    token_id: tokenId, // Base token ID (wrap.near) for intents.near
    amount: totalAmount.toString(),
    msg: JSON.stringify({ list_id: listId }), // Include list_id in message
  },
  gas: '300000000000000',
  attachedDeposit: '1',
});

console.log(`✅ Payment list approved`);

// Step 12: Wait for processing
console.log('\n⏳ Waiting for payment processing...');
let allProcessed = false;
let attempts = 0;
const maxAttempts = 60;

while (!allProcessed && attempts < maxAttempts) {
  await sleep(5000);
  attempts++;
  
  const listStatus = await viewPaymentList(genesisAccount, listId);
  const processedCount = listStatus.payments.filter(p => 
    p.status && p.status.Paid && typeof p.status.Paid.block_height === 'number'
  ).length;
  
  const progress = ((processedCount / listStatus.payments.length) * 100).toFixed(1);
  console.log(`📊 Progress: ${processedCount}/${listStatus.payments.length} (${progress}%)`);
  
  if (processedCount === listStatus.payments.length) {
    allProcessed = true;
  }
}

assert.equal(allProcessed, true, 'All payments must complete within timeout');

// Step 13: Verify all payments have block_height
console.log('\n🔍 Verifying all payments have block_height...');
const finalStatus = await viewPaymentList(genesisAccount, listId);

const paymentsWithBlockHeight = finalStatus.payments.filter(p => 
  p.status && p.status.Paid && typeof p.status.Paid.block_height === 'number'
);

console.log(`📊 Payments with block_height: ${paymentsWithBlockHeight.length}/${finalStatus.payments.length}`);

assert.equal(
  paymentsWithBlockHeight.length, 
  totalRecipients, 
  `All ${totalRecipients} payments must have block_height registered`
);
console.log(`✅ All payments have block_height registered`);

// Step 14: Verify transactions and receipts
console.log('\n🔗 Verifying payment transactions and receipts...');

const rpcClient = new NearRpcClient({ endpoint: CONFIG.SANDBOX_RPC_URL });
let successfulTransfers = [];
let failedTransfers = [];

for (const payment of finalStatus.payments) {
  const recipient = payment.recipient;
  const isRegistered = registeredRecipients.includes(recipient);
  
  console.log(`\n📦 Checking ${isRegistered ? 'REGISTERED' : 'NON-REGISTERED'}: ${recipient.substring(0, 20)}...`);
  
  // Get transaction hash from API
  const txResponse = await apiRequest(`/list/${listId}/transaction/${recipient}`);
  assert.equal(txResponse.success, true, `Must be able to get transaction for ${recipient}`);
  
  const txHash = txResponse.transaction_hash;
  console.log(`   Transaction hash: ${txHash.substring(0, 16)}...`);
  
  // Get transaction status
  const txStatus = await rpcTx(rpcClient, { txHash, senderAccountId: CONFIG.BULK_PAYMENT_CONTRACT_ID });
  
  // Check if THIS specific recipient has a failed receipt
  // In batched transactions, multiple recipients share the same transaction,
  // so we must filter for failures related to this specific recipient
  const recipientFailedReceipt = txStatus.receiptsOutcome.find(ro => {
    if (!ro.outcome.status?.Failure) return false;
    
    const failure = ro.outcome.status.Failure;
    
    // Check if the failure is for this specific recipient by looking at:
    // 1. The accountId in AccountDoesNotExist errors
    // 2. The receiver_id field on the receipt outcome
    const accountId = failure?.ActionError?.kind?.AccountDoesNotExist?.accountId;
    if (accountId === recipient) return true;
    
    // Also check receiver_id on the outcome
    if (ro.outcome.executor_id === recipient || ro.outcome.receiver_id === recipient) {
      return true;
    }
    
    return false;
  });
  
  if (recipientFailedReceipt) {
    console.log(`   ❌ Transaction failed for this recipient`);
    console.log(`      Failure: ${JSON.stringify(recipientFailedReceipt.outcome.status.Failure)}`);
    failedTransfers.push({ recipient, isRegistered, txHash, failure: recipientFailedReceipt.outcome.status.Failure });
    
    // Fail immediately if a registered account has failed transfer (unexpected)
    if (isRegistered) {
      assert.fail(`Unexpected failure for registered account ${recipient}: ${JSON.stringify(recipientFailedReceipt.outcome.status.Failure)}`);
    }
  } else {
    console.log(`   ✅ Transaction succeeded for this recipient`);
    successfulTransfers.push({ recipient, isRegistered, txHash });
    
    // Fail immediately if a non-registered account has successful transfer (unexpected)
    if (!isRegistered) {
      assert.fail(`Unexpected success for non-registered account ${recipient}`);
    }
  }
}

// Step 15: Verify balance changes for registered accounts
console.log('\n💰 Verifying token balance changes...');

for (const recipient of registeredRecipients) {
  const balance = await getMultiTokenBalance(genesisAccount, recipient, tokenId);
  const payment = payments.find(p => p.recipient === recipient);
  
  console.log(`✅ Registered ${recipient.substring(0, 16)}...: balance = ${balance}`);
  assert.ok(BigInt(balance) >= BigInt(payment.amount), 
    `Registered account ${recipient} must have balance >= ${payment.amount}, got ${balance}`);
}

for (const recipient of nonRegisteredRecipients) {
  const balance = await getMultiTokenBalance(genesisAccount, recipient, tokenId);
  console.log(`ℹ️  Non-registered ${recipient.substring(0, 16)}...: balance = ${balance}`);
  assert.equal(balance, '0', 
    `Non-registered account ${recipient} must have 0 balance, got ${balance}`);
}

// Step 16: Validate expectations
console.log('\n=====================================');
console.log('📊 Test Summary');
console.log('=====================================');
console.log(`Total Recipients: ${totalRecipients}`);
console.log(`  - Registered: ${CONFIG.NUM_REGISTERED}`);
console.log(`  - Non-Registered: ${CONFIG.NUM_NON_REGISTERED}`);
console.log(`Payments with block_height: ${paymentsWithBlockHeight.length}`);
console.log(`Successful transfers: ${successfulTransfers.length}`);
console.log(`Failed transfers: ${failedTransfers.length}`);
console.log('=====================================\n');

// Assertions based on requirements
assert.equal(paymentsWithBlockHeight.length, totalRecipients, 
  'All payments must be processed with block_height');

// Registered accounts should have successful transfers
const registeredSuccesses = successfulTransfers.filter(t => t.isRegistered).length;
assert.equal(registeredSuccesses, CONFIG.NUM_REGISTERED, 
  'All registered accounts must have successful transfers');

// Non-registered accounts should have failed transfers
const nonRegisteredFailures = failedTransfers.filter(t => !t.isRegistered).length;
assert.equal(nonRegisteredFailures, CONFIG.NUM_NON_REGISTERED, 
  'All non-registered accounts must have failed transfers');

console.log('🎉 Test PASSED: NEAR Intents payments behave correctly for non-registered accounts!');
console.log('   ✅ All payments marked as processed');
console.log('   ✅ Registered accounts received tokens successfully');
console.log('   ✅ Non-registered accounts have failed receipts');
process.exit(0);

} catch (error) {
  console.error('❌ Test FAILED:', error.message);
  if (error.stack) {
    console.error(error.stack);
  }
  process.exit(1);
}
