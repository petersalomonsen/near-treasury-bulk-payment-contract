// Integration tests for NEAR Treasury Bulk Payment Contract
// NOTE: These tests require near-workspaces sandbox which downloads near-sandbox binary.
// In CI/sandboxed environments without internet access, these tests may fail to build.
// The contract itself compiles and works correctly - these are end-to-end integration tests.

use near_sdk::NearToken;
use near_workspaces::{Account, Contract};
use serde_json::json;

const WASM_PATH: &str = "./target/wasm32-unknown-unknown/release/near_treasury_bulk_payment_contract.wasm";

#[tokio::test]
async fn test_storage_purchase() -> Result<(), Box<dyn std::error::Error>> {
    let worker = near_workspaces::sandbox().await?;
    let wasm = std::fs::read(WASM_PATH)?;
    let contract = worker.dev_deploy(&wasm).await?;

    let user = worker.dev_create_account().await?;

    // Calculate expected cost for 10 records
    // 216 bytes per record * 10 = 2160 bytes
    // 2160 * 10^19 yoctoNEAR/byte = 21600000000000000000000 yoctoNEAR
    // With 10% markup: 21600000000000000000000 * 1.1 = 23760000000000000000000 yoctoNEAR
    let num_records = 10;
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);

    // Get initial contract balance
    let initial_balance = contract.view_account().await?.balance;

    // Buy storage
    let result = user
        .call(contract.id(), "buy_storage")
        .args_json(json!({ "num_records": num_records }))
        .deposit(storage_cost)
        .transact()
        .await?;

    assert!(result.is_success(), "Storage purchase should succeed");

    // Verify contract balance increased (revenue)
    let final_balance = contract.view_account().await?.balance;
    assert!(
        final_balance > initial_balance,
        "Contract balance should increase (revenue generation)"
    );

    // Verify storage credits
    let credits: NearToken = contract
        .view("view_storage_credits")
        .args_json(json!({ "account_id": user.id() }))
        .await?
        .json()?;

    assert_eq!(
        credits.as_yoctonear(),
        num_records,
        "Storage credits should be tracked"
    );

    Ok(())
}

#[tokio::test]
async fn test_submit_and_approve_list() -> Result<(), Box<dyn std::error::Error>> {
    let worker = near_workspaces::sandbox().await?;
    let wasm = std::fs::read(WASM_PATH)?;
    let contract = worker.dev_deploy(&wasm).await?;

    let user = worker.dev_create_account().await?;
    let recipient1 = worker.dev_create_account().await?;
    let recipient2 = worker.dev_create_account().await?;

    // Buy storage first
    let num_records = 5;
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
    
    user.call(contract.id(), "buy_storage")
        .args_json(json!({ "num_records": num_records }))
        .deposit(storage_cost)
        .transact()
        .await?;

    // Submit payment list
    let payments = vec![
        json!({
            "recipient": recipient1.id().to_string(),
            "amount": "1000000000000000000000000" // 1 NEAR
        }),
        json!({
            "recipient": recipient2.id().to_string(),
            "amount": "2000000000000000000000000" // 2 NEAR
        }),
    ];

    let submit_result = user
        .call(contract.id(), "submit_list")
        .args_json(json!({
            "token_id": "native",
            "payments": payments
        }))
        .transact()
        .await?;

    assert!(submit_result.is_success(), "Submit list should succeed");
    let list_id: u64 = submit_result.json()?;

    // Verify storage credits were deducted
    let credits: NearToken = contract
        .view("view_storage_credits")
        .args_json(json!({ "account_id": user.id() }))
        .await?
        .json()?;

    assert_eq!(
        credits.as_yoctonear(),
        num_records - 2,
        "Storage credits should be deducted"
    );

    // Approve the list with exact deposit
    let total_amount = NearToken::from_yoctonear(3_000_000_000_000_000_000_000_000); // 3 NEAR
    let approve_result = user
        .call(contract.id(), "approve_list")
        .args_json(json!({ "list_ref": list_id }))
        .deposit(total_amount)
        .transact()
        .await?;

    assert!(approve_result.is_success(), "Approve list should succeed");

    // View the list to verify status
    let list: serde_json::Value = contract
        .view("view_list")
        .args_json(json!({ "list_ref": list_id }))
        .await?
        .json()?;

    assert_eq!(list["status"], "Approved");

    Ok(())
}

#[tokio::test]
async fn test_batch_processing() -> Result<(), Box<dyn std::error::Error>> {
    let worker = near_workspaces::sandbox().await?;
    let wasm = std::fs::read(WASM_PATH)?;
    let contract = worker.dev_deploy(&wasm).await?;

    let user = worker.dev_create_account().await?;

    // Buy storage for 250 payments
    let num_records = 250;
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000 * 25);
    
    user.call(contract.id(), "buy_storage")
        .args_json(json!({ "num_records": num_records }))
        .deposit(storage_cost)
        .transact()
        .await?;

    // Create 250 payment entries
    let mut payments = Vec::new();
    let mut total_amount = 0u128;
    for i in 0..250 {
        let recipient = worker.dev_create_account().await?;
        let amount = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR
        payments.push(json!({
            "recipient": recipient.id().to_string(),
            "amount": amount.to_string()
        }));
        total_amount += amount;
    }

    // Submit large payment list
    let submit_result = user
        .call(contract.id(), "submit_list")
        .args_json(json!({
            "token_id": "native",
            "payments": payments
        }))
        .transact()
        .await?;

    let list_id: u64 = submit_result.json()?;

    // Approve the list
    let approve_result = user
        .call(contract.id(), "approve_list")
        .args_json(json!({ "list_ref": list_id }))
        .deposit(NearToken::from_yoctonear(total_amount))
        .transact()
        .await?;

    assert!(approve_result.is_success(), "Approve large list should succeed");

    // Process first batch (100 payments)
    let payout1_result = user
        .call(contract.id(), "payout_batch")
        .args_json(json!({
            "list_ref": list_id,
            "max_payments": 100
        }))
        .transact()
        .await?;

    assert!(payout1_result.is_success(), "First batch should succeed");

    // Process second batch (100 payments)
    let payout2_result = user
        .call(contract.id(), "payout_batch")
        .args_json(json!({
            "list_ref": list_id,
            "max_payments": 100
        }))
        .transact()
        .await?;

    assert!(payout2_result.is_success(), "Second batch should succeed");

    // Process third batch (50 payments)
    let payout3_result = user
        .call(contract.id(), "payout_batch")
        .args_json(json!({
            "list_ref": list_id,
            "max_payments": 100
        }))
        .transact()
        .await?;

    assert!(payout3_result.is_success(), "Third batch should succeed");

    Ok(())
}

#[tokio::test]
async fn test_failed_payment_retry() -> Result<(), Box<dyn std::error::Error>> {
    let worker = near_workspaces::sandbox().await?;
    let wasm = std::fs::read(WASM_PATH)?;
    let contract = worker.dev_deploy(&wasm).await?;

    let user = worker.dev_create_account().await?;

    // Buy storage
    let num_records = 10;
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
    
    user.call(contract.id(), "buy_storage")
        .args_json(json!({ "num_records": num_records }))
        .deposit(storage_cost)
        .transact()
        .await?;

    // Submit payment list with unsupported token (will fail)
    let recipient = worker.dev_create_account().await?;
    let payments = vec![
        json!({
            "recipient": recipient.id().to_string(),
            "amount": "1000000000000000000000000"
        }),
    ];

    let submit_result = user
        .call(contract.id(), "submit_list")
        .args_json(json!({
            "token_id": "unsupported_token",
            "payments": payments
        }))
        .transact()
        .await?;

    let list_id: u64 = submit_result.json()?;

    // Approve the list
    let total_amount = NearToken::from_yoctonear(1_000_000_000_000_000_000_000_000);
    user.call(contract.id(), "approve_list")
        .args_json(json!({ "list_ref": list_id }))
        .deposit(total_amount)
        .transact()
        .await?;

    // Process payments (should fail due to unsupported token)
    let payout_result = user
        .call(contract.id(), "payout_batch")
        .args_json(json!({ "list_ref": list_id }))
        .transact()
        .await?;

    assert!(payout_result.is_success(), "Payout batch call should succeed");

    // View list to verify Failed status
    let list: serde_json::Value = contract
        .view("view_list")
        .args_json(json!({ "list_ref": list_id }))
        .await?
        .json()?;

    assert_eq!(list["payments"][0]["status"]["Failed"]["error"], "Unsupported token type");

    // Retry failed payments
    let retry_result = user
        .call(contract.id(), "retry_failed")
        .args_json(json!({ "list_ref": list_id }))
        .transact()
        .await?;

    assert!(retry_result.is_success(), "Retry should succeed");

    // Verify status is back to Pending
    let list_after: serde_json::Value = contract
        .view("view_list")
        .args_json(json!({ "list_ref": list_id }))
        .await?
        .json()?;

    assert_eq!(list_after["payments"][0]["status"], "Pending");

    Ok(())
}

#[tokio::test]
async fn test_reject_list_with_refund() -> Result<(), Box<dyn std::error::Error>> {
    let worker = near_workspaces::sandbox().await?;
    let wasm = std::fs::read(WASM_PATH)?;
    let contract = worker.dev_deploy(&wasm).await?;

    let user = worker.dev_create_account().await?;
    let recipient = worker.dev_create_account().await?;

    // Buy storage
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
    user.call(contract.id(), "buy_storage")
        .args_json(json!({ "num_records": 5 }))
        .deposit(storage_cost)
        .transact()
        .await?;

    // Submit and approve list
    let payments = vec![json!({
        "recipient": recipient.id().to_string(),
        "amount": "1000000000000000000000000"
    })];

    let submit_result = user
        .call(contract.id(), "submit_list")
        .args_json(json!({
            "token_id": "native",
            "payments": payments
        }))
        .transact()
        .await?;

    let list_id: u64 = submit_result.json()?;

    // Get user balance before approval
    let balance_before_approve = user.view_account().await?.balance;

    // Approve
    let total_amount = NearToken::from_yoctonear(1_000_000_000_000_000_000_000_000);
    user.call(contract.id(), "approve_list")
        .args_json(json!({ "list_ref": list_id }))
        .deposit(total_amount)
        .transact()
        .await?;

    // Get user balance after approval
    let balance_after_approve = user.view_account().await?.balance;
    assert!(
        balance_after_approve < balance_before_approve,
        "User balance should decrease after approval"
    );

    // Reject the list
    let reject_result = user
        .call(contract.id(), "reject_list")
        .args_json(json!({ "list_ref": list_id }))
        .transact()
        .await?;

    assert!(reject_result.is_success(), "Reject should succeed");

    // Get user balance after rejection
    let balance_after_reject = user.view_account().await?.balance;
    
    // Balance should be approximately restored (minus gas fees)
    // We check that the difference is less than 0.1 NEAR (gas fees)
    let balance_diff = (balance_before_approve.as_yoctonear() as i128 
        - balance_after_reject.as_yoctonear() as i128).abs();
    assert!(
        balance_diff < 100_000_000_000_000_000_000_000, // 0.1 NEAR
        "Balance should be approximately restored after refund"
    );

    Ok(())
}

#[tokio::test]
async fn test_revenue_generation() -> Result<(), Box<dyn std::error::Error>> {
    let worker = near_workspaces::sandbox().await?;
    let wasm = std::fs::read(WASM_PATH)?;
    let contract = worker.dev_deploy(&wasm).await?;

    let user1 = worker.dev_create_account().await?;
    let user2 = worker.dev_create_account().await?;
    let user3 = worker.dev_create_account().await?;

    // Get initial contract balance
    let initial_balance = contract.view_account().await?.balance;

    // Multiple users buy storage
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
    
    for user in [&user1, &user2, &user3] {
        user.call(contract.id(), "buy_storage")
            .args_json(json!({ "num_records": 10 }))
            .deposit(storage_cost)
            .transact()
            .await?;
    }

    // Get final contract balance
    let final_balance = contract.view_account().await?.balance;

    // Calculate expected revenue
    // 10% markup on 3 purchases of 10 records each
    let base_cost = NearToken::from_yoctonear(21_600_000_000_000_000_000_000);
    let markup = NearToken::from_yoctonear(2_160_000_000_000_000_000_000);
    let expected_revenue = NearToken::from_yoctonear(markup.as_yoctonear() * 3);

    let actual_revenue = NearToken::from_yoctonear(
        final_balance.as_yoctonear() - initial_balance.as_yoctonear()
    );

    // Verify revenue is at least the expected markup (may be slightly more due to gas refunds)
    assert!(
        actual_revenue.as_yoctonear() >= expected_revenue.as_yoctonear(),
        "Contract should generate revenue from storage markup. Expected at least: {}, Got: {}",
        expected_revenue,
        actual_revenue
    );

    Ok(())
}

#[tokio::test]
async fn test_exact_deposit_validation() -> Result<(), Box<dyn std::error::Error>> {
    let worker = near_workspaces::sandbox().await?;
    let wasm = std::fs::read(WASM_PATH)?;
    let contract = worker.dev_deploy(&wasm).await?;

    let user = worker.dev_create_account().await?;

    // Try to buy storage with wrong deposit amount (should fail)
    let wrong_deposit = NearToken::from_yoctonear(1_000_000_000_000_000_000_000_000);
    let result = user
        .call(contract.id(), "buy_storage")
        .args_json(json!({ "num_records": 10 }))
        .deposit(wrong_deposit)
        .transact()
        .await;

    assert!(result.is_err() || !result.unwrap().is_success(), 
        "Should fail with wrong deposit amount");

    Ok(())
}

#[tokio::test]
async fn test_unauthorized_operations() -> Result<(), Box<dyn std::error::Error>> {
    let worker = near_workspaces::sandbox().await?;
    let wasm = std::fs::read(WASM_PATH)?;
    let contract = worker.dev_deploy(&wasm).await?;

    let user = worker.dev_create_account().await?;
    let attacker = worker.dev_create_account().await?;
    let recipient = worker.dev_create_account().await?;

    // Setup: user buys storage and submits list
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
    user.call(contract.id(), "buy_storage")
        .args_json(json!({ "num_records": 10 }))
        .deposit(storage_cost)
        .transact()
        .await?;

    let payments = vec![json!({
        "recipient": recipient.id().to_string(),
        "amount": "1000000000000000000000000"
    })];

    let submit_result = user
        .call(contract.id(), "submit_list")
        .args_json(json!({
            "token_id": "native",
            "payments": payments
        }))
        .transact()
        .await?;

    let list_id: u64 = submit_result.json()?;

    // Attacker tries to approve the list (should fail)
    let total_amount = NearToken::from_yoctonear(1_000_000_000_000_000_000_000_000);
    let result = attacker
        .call(contract.id(), "approve_list")
        .args_json(json!({ "list_ref": list_id }))
        .deposit(total_amount)
        .transact()
        .await;

    assert!(result.is_err() || !result.unwrap().is_success(), 
        "Attacker should not be able to approve list");

    Ok(())
}
