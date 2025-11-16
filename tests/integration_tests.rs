// Integration tests for NEAR Treasury Bulk Payment Contract
// Uses near-sandbox and near-api instead of near-workspaces

use near_sdk::{serde_json::json, AccountId, NearToken};

fn get_genesis_signer() -> std::sync::Arc<near_api::Signer> {
    near_api::Signer::new(near_api::Signer::from_secret_key(
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY
            .parse()
            .unwrap(),
    ))
    .unwrap()
}

async fn create_account(
    new_account_id: &AccountId,
    balance: NearToken,
    network_config: &near_api::NetworkConfig,
) -> std::sync::Arc<near_api::Signer> {
    near_api::Account::create_account(new_account_id.clone())
        .fund_myself(
            new_account_id.get_parent_account_id().unwrap().to_owned(),
            balance,
        )
        .public_key(
            near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PUBLIC_KEY
                .parse::<near_api::PublicKey>()
                .unwrap(),
        )
        .unwrap()
        .with_signer(get_genesis_signer())
        .send_to(network_config)
        .await
        .unwrap()
        .assert_success();
    // We use the same keypair for the new account as the genesis account
    get_genesis_signer()
}

async fn setup_contract(
) -> Result<(near_sandbox::Sandbox, near_api::NetworkConfig, AccountId), Box<dyn std::error::Error>>
{
    let sandbox = near_sandbox::Sandbox::start_sandbox_with_version("2.7.1").await?;
    let network_config = near_api::NetworkConfig {
        network_name: "sandbox".to_string(),
        rpc_endpoints: vec![near_api::RPCEndpoint::new(
            sandbox.rpc_addr.parse().unwrap(),
        )],
        linkdrop_account_id: None,
        ..near_api::NetworkConfig::testnet()
    };

    // Build the contract
    let contract_wasm_path = cargo_near_build::build_with_cli(Default::default())?;

    // Deploy contract
    let contract_id: AccountId = format!(
        "bulk-payment.{}",
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT
    )
    .parse()
    .unwrap();
    let contract_signer =
        create_account(&contract_id, NearToken::from_near(50), &network_config).await;

    near_api::Contract::deploy(contract_id.clone())
        .use_code(std::fs::read(contract_wasm_path).unwrap())
        .with_init_call("new", ())
        .unwrap()
        .with_signer(contract_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    Ok((sandbox, network_config, contract_id))
}

#[tokio::test]
async fn test_storage_purchase() -> Result<(), Box<dyn std::error::Error>> {
    let (_sandbox, network_config, contract_id) = setup_contract().await?;

    // Create user account
    let user_id: AccountId = format!("user.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let user_signer = create_account(&user_id, NearToken::from_near(50), &network_config).await;

    // Calculate expected cost for 10 records
    let num_records = 10;
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);

    // Get initial contract balance
    let initial_balance = near_api::Account(contract_id.clone())
        .view()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data
        .amount;

    // Buy storage
    near_api::Contract(contract_id.clone())
        .call_function("buy_storage", json!({ "num_records": num_records }))
        .unwrap()
        .transaction()
        .deposit(storage_cost)
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Verify contract balance increased (revenue)
    let final_balance = near_api::Account(contract_id.clone())
        .view()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data
        .amount;

    assert!(
        final_balance > initial_balance,
        "Contract balance should increase (revenue generation)"
    );

    // Verify storage credits
    let credits: NearToken = near_api::Contract(contract_id.clone())
        .call_function(
            "view_storage_credits",
            json!({ "account_id": user_id }),
        )
        .unwrap()
        .read_only()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data;

    assert_eq!(
        credits.as_yoctonear(), num_records as u128,
        "Storage credits should be tracked"
    );

    Ok(())
}

#[tokio::test]
async fn test_submit_and_approve_list() -> Result<(), Box<dyn std::error::Error>> {
    let (_sandbox, network_config, contract_id) = setup_contract().await?;

    let user_id: AccountId = format!("user.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let user_signer = create_account(&user_id, NearToken::from_near(50), &network_config).await;

    let recipient1: AccountId = format!("recipient1.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let recipient2: AccountId = format!("recipient2.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();

    // Buy storage first
    let num_records = 5;
    let storage_cost = NearToken::from_yoctonear(11_880_000_000_000_000_000_000);

    near_api::Contract(contract_id.clone())
        .call_function("buy_storage", json!({ "num_records": num_records }))
        .unwrap()
        .transaction()
        .deposit(storage_cost)
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Submit payment list
    let payments = vec![
        json!({
            "recipient": recipient1.to_string(),
            "amount": "1000000000000000000000000" // 1 NEAR
        }),
        json!({
            "recipient": recipient2.to_string(),
            "amount": "2000000000000000000000000" // 2 NEAR
        }),
    ];

    let submit_result = near_api::Contract(contract_id.clone())
        .call_function(
            "submit_list",
            json!({
                "token_id": "native",
                "payments": payments
            }),
        )
        .unwrap()
        .transaction()
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap();

    submit_result.assert_success();
    // List IDs start from 0
    let list_id: u64 = 0;

    // Verify storage credits were deducted
    let credits: NearToken = near_api::Contract(contract_id.clone())
        .call_function(
            "view_storage_credits",
            json!({ "account_id": user_id }),
        )
        .unwrap()
        .read_only()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data;

    assert_eq!(
        credits.as_yoctonear(),
        (num_records - 2) as u128,
        "Storage credits should be deducted"
    );

    // Approve the list with exact deposit
    let total_amount = NearToken::from_yoctonear(3_000_000_000_000_000_000_000_000); // 3 NEAR
    near_api::Contract(contract_id.clone())
        .call_function("approve_list", json!({ "list_ref": list_id }))
        .unwrap()
        .transaction()
        .deposit(total_amount)
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // View the list to verify status
    let list: serde_json::Value = near_api::Contract(contract_id.clone())
        .call_function("view_list", json!({ "list_ref": list_id }))
        .unwrap()
        .read_only()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data;

    assert_eq!(list["status"], "Approved");

    Ok(())
}

#[tokio::test]
async fn test_batch_processing() -> Result<(), Box<dyn std::error::Error>> {
    let (_sandbox, network_config, contract_id) = setup_contract().await?;

    let user_id: AccountId = format!("user.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let user_signer = create_account(&user_id, NearToken::from_near(300), &network_config).await;

    // Buy storage for 250 payments (need 250 credits, buy 260 to be safe)
    let num_records = 260;
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000 * 26);

    near_api::Contract(contract_id.clone())
        .call_function("buy_storage", json!({ "num_records": num_records }))
        .unwrap()
        .transaction()
        .deposit(storage_cost)
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Create 250 payment entries
    let mut payments = Vec::new();
    for i in 0..250 {
        let recipient: AccountId = format!(
            "recipient{}.{}",
            i,
            near_sandbox::config::DEFAULT_GENESIS_ACCOUNT
        )
        .parse()
        .unwrap();
        payments.push(json!({
            "recipient": recipient.to_string(),
            "amount": "1000000000000000000000000" // 1 NEAR
        }));
    }

    // Submit large payment list
    let submit_result = near_api::Contract(contract_id.clone())
        .call_function(
            "submit_list",
            json!({
                "token_id": "native",
                "payments": payments
            }),
        )
        .unwrap()
        .transaction()
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap();

    submit_result.assert_success();
    // List IDs start from 0 and increment
    let list_id: u64 = 0;

    // Approve the list
    let total_amount = NearToken::from_yoctonear(250_000_000_000_000_000_000_000_000); // 250 NEAR
    near_api::Contract(contract_id.clone())
        .call_function("approve_list", json!({ "list_ref": list_id }))
        .unwrap()
        .transaction()
        .deposit(total_amount)
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Process first batch (100 payments)
    near_api::Contract(contract_id.clone())
        .call_function(
            "payout_batch",
            json!({
                "list_ref": list_id,
                "max_payments": 100
            }),
        )
        .unwrap()
        .transaction()
        .gas(near_sdk::Gas::from_tgas(300))
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Process second batch (100 payments)
    near_api::Contract(contract_id.clone())
        .call_function(
            "payout_batch",
            json!({
                "list_ref": list_id,
                "max_payments": 100
            }),
        )
        .unwrap()
        .transaction()
        .gas(near_sdk::Gas::from_tgas(300))
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Process third batch (50 payments)
    near_api::Contract(contract_id.clone())
        .call_function(
            "payout_batch",
            json!({
                "list_ref": list_id,
                "max_payments": 100
            }),
        )
        .unwrap()
        .transaction()
        .gas(near_sdk::Gas::from_tgas(300))
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    Ok(())
}

#[tokio::test]
async fn test_failed_payment_retry() -> Result<(), Box<dyn std::error::Error>> {
    let (_sandbox, network_config, contract_id) = setup_contract().await?;

    let user_id: AccountId = format!("user.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let user_signer = create_account(&user_id, NearToken::from_near(50), &network_config).await;

    // Buy storage
    let num_records = 10;
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);

    near_api::Contract(contract_id.clone())
        .call_function("buy_storage", json!({ "num_records": num_records }))
        .unwrap()
        .transaction()
        .deposit(storage_cost)
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Submit payment list with unsupported token (will fail)
    let recipient: AccountId = format!(
        "recipient.{}",
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT
    )
    .parse()
    .unwrap();
    let payments = vec![json!({
        "recipient": recipient.to_string(),
        "amount": "1000000000000000000000000"
    })];

    let submit_result = near_api::Contract(contract_id.clone())
        .call_function(
            "submit_list",
            json!({
                "token_id": "unsupported_token",
                "payments": payments
            }),
        )
        .unwrap()
        .transaction()
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap();

    submit_result.assert_success();
    // List IDs start from 0 and increment
    let list_id: u64 = 0;

    // Approve the list
    let total_amount = NearToken::from_yoctonear(1_000_000_000_000_000_000_000_000);
    near_api::Contract(contract_id.clone())
        .call_function("approve_list", json!({ "list_ref": list_id }))
        .unwrap()
        .transaction()
        .deposit(total_amount)
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Process payments (should fail due to unsupported token)
    near_api::Contract(contract_id.clone())
        .call_function("payout_batch", json!({ "list_ref": list_id }))
        .unwrap()
        .transaction()
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // View list to verify Failed status
    let list: serde_json::Value = near_api::Contract(contract_id.clone())
        .call_function("view_list", json!({ "list_ref": list_id }))
        .unwrap()
        .read_only()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data;

    assert_eq!(
        list["payments"][0]["status"]["Failed"]["error"],
        "Unsupported token type"
    );

    // Retry failed payments
    near_api::Contract(contract_id.clone())
        .call_function("retry_failed", json!({ "list_ref": list_id }))
        .unwrap()
        .transaction()
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Verify status is back to Pending
    let list_after: serde_json::Value = near_api::Contract(contract_id.clone())
        .call_function("view_list", json!({ "list_ref": list_id }))
        .unwrap()
        .read_only()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data;

    assert_eq!(list_after["payments"][0]["status"], "Pending");

    Ok(())
}

#[tokio::test]
async fn test_reject_pending_list() -> Result<(), Box<dyn std::error::Error>> {
    let (_sandbox, network_config, contract_id) = setup_contract().await?;

    let user_id: AccountId = format!("user.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let user_signer = create_account(&user_id, NearToken::from_near(50), &network_config).await;

    let recipient: AccountId = format!(
        "recipient.{}",
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT
    )
    .parse()
    .unwrap();

    // Buy storage
    let storage_cost = NearToken::from_yoctonear(11_880_000_000_000_000_000_000);
    near_api::Contract(contract_id.clone())
        .call_function("buy_storage", json!({ "num_records": 5 }))
        .unwrap()
        .transaction()
        .deposit(storage_cost)
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Submit list (but don't approve)
    let payments = vec![json!({
        "recipient": recipient.to_string(),
        "amount": "1000000000000000000000000"
    })];

    near_api::Contract(contract_id.clone())
        .call_function(
            "submit_list",
            json!({
                "token_id": "native",
                "payments": payments
            }),
        )
        .unwrap()
        .transaction()
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // List IDs start from 0 and increment
    let list_id: u64 = 0;

    // Reject the pending list
    near_api::Contract(contract_id.clone())
        .call_function("reject_list", json!({ "list_ref": list_id }))
        .unwrap()
        .transaction()
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Verify list is rejected
    let list: serde_json::Value = near_api::Contract(contract_id.clone())
        .call_function("view_list", json!({ "list_ref": list_id }))
        .unwrap()
        .read_only()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data;

    assert_eq!(list["status"], "Rejected");

    Ok(())
}

#[tokio::test]
async fn test_revenue_generation() -> Result<(), Box<dyn std::error::Error>> {
    let (_sandbox, network_config, contract_id) = setup_contract().await?;

    let user1: AccountId = format!("user1.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let user1_signer = create_account(&user1, NearToken::from_near(50), &network_config).await;

    let user2: AccountId = format!("user2.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let user2_signer = create_account(&user2, NearToken::from_near(50), &network_config).await;

    let user3: AccountId = format!("user3.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let user3_signer = create_account(&user3, NearToken::from_near(50), &network_config).await;

    // Get initial contract balance
    let initial_balance = near_api::Account(contract_id.clone())
        .view()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data
        .amount;

    // Multiple users buy storage
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);

    for (user, signer) in [
        (user1.clone(), user1_signer),
        (user2.clone(), user2_signer),
        (user3.clone(), user3_signer),
    ] {
        near_api::Contract(contract_id.clone())
            .call_function("buy_storage", json!({ "num_records": 10 }))
            .unwrap()
            .transaction()
            .deposit(storage_cost)
            .with_signer(user, signer)
            .send_to(&network_config)
            .await
            .unwrap()
            .assert_success();
    }

    // Get final contract balance
    let final_balance = near_api::Account(contract_id.clone())
        .view()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data
        .amount;

    // Calculate expected revenue
    // 10% markup on 3 purchases of 10 records each
    let markup = NearToken::from_yoctonear(2_160_000_000_000_000_000_000);
    let expected_revenue = NearToken::from_yoctonear(markup.as_yoctonear() * 3);

    let actual_revenue = NearToken::from_yoctonear(
        final_balance.as_yoctonear().saturating_sub(initial_balance.as_yoctonear())
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
    let (_sandbox, network_config, contract_id) = setup_contract().await?;

    let user_id: AccountId = format!("user.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let user_signer = create_account(&user_id, NearToken::from_near(50), &network_config).await;

    // Try to buy storage with wrong deposit amount (should fail)
    let wrong_deposit = NearToken::from_yoctonear(1_000_000_000_000_000_000_000_000);
    let result = near_api::Contract(contract_id.clone())
        .call_function("buy_storage", json!({ "num_records": 10 }))
        .unwrap()
        .transaction()
        .deposit(wrong_deposit)
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await;

    // Should fail with execution error
    assert!(
        result.is_err() || !result.unwrap().is_success(),
        "Should fail with wrong deposit amount"
    );

    Ok(())
}

#[tokio::test]
async fn test_unauthorized_operations() -> Result<(), Box<dyn std::error::Error>> {
    let (_sandbox, network_config, contract_id) = setup_contract().await?;

    let user_id: AccountId = format!("user.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let user_signer = create_account(&user_id, NearToken::from_near(50), &network_config).await;

    let attacker: AccountId = format!("attacker.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let attacker_signer =
        create_account(&attacker, NearToken::from_near(50), &network_config).await;

    let recipient: AccountId = format!(
        "recipient.{}",
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT
    )
    .parse()
    .unwrap();

    // Setup: user buys storage and submits list
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000);
    near_api::Contract(contract_id.clone())
        .call_function("buy_storage", json!({ "num_records": 10 }))
        .unwrap()
        .transaction()
        .deposit(storage_cost)
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    let payments = vec![json!({
        "recipient": recipient.to_string(),
        "amount": "1000000000000000000000000"
    })];

    let submit_result = near_api::Contract(contract_id.clone())
        .call_function(
            "submit_list",
            json!({
                "token_id": "native",
                "payments": payments
            }),
        )
        .unwrap()
        .transaction()
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap();

    submit_result.assert_success();
    // List IDs start from 0 and increment
    let list_id: u64 = 0;

    // Attacker tries to approve the list (should fail)
    let total_amount = NearToken::from_yoctonear(1_000_000_000_000_000_000_000_000);
    let result = near_api::Contract(contract_id.clone())
        .call_function("approve_list", json!({ "list_ref": list_id }))
        .unwrap()
        .transaction()
        .deposit(total_amount)
        .with_signer(attacker.clone(), attacker_signer.clone())
        .send_to(&network_config)
        .await;

    assert!(
        result.is_err() || !result.unwrap().is_success(),
        "Attacker should not be able to approve list"
    );

    Ok(())
}
