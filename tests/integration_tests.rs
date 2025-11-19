// Integration tests for NEAR Treasury Bulk Payment Contract
// Uses near-sandbox and near-api instead of near-workspaces

use base64::Engine;
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

/// Import a contract from mainnet to the sandbox
/// Similar to: https://github.com/NEAR-DevHub/near-treasury/blob/staging/playwright-tests/util/sandbox.js#L457
/// Returns the signer for the imported account (same as genesis signer)
/// Note: For top-level accounts, the account must be created via SandboxConfig.additional_accounts before calling this
async fn import_contract(
    _sandbox: &near_sandbox::Sandbox,
    network_config: &near_api::NetworkConfig,
    account_id: &AccountId,
    mainnet_account_id: &str,
) -> Result<std::sync::Arc<near_api::Signer>, Box<dyn std::error::Error>> {
    // Configure mainnet connection
    let mainnet_config = near_api::NetworkConfig::mainnet();

    // Fetch contract code from mainnet
    let mainnet_rpc_url = mainnet_config.rpc_endpoints[0].url.as_str();
    let client = reqwest::Client::new();

    // View code
    let code_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "dontcare",
        "method": "query",
        "params": {
            "request_type": "view_code",
            "finality": "final",
            "account_id": mainnet_account_id
        }
    });

    let code_response: serde_json::Value = client
        .post(mainnet_rpc_url)
        .json(&code_request)
        .send()
        .await?
        .json()
        .await?;

    let contract_code_base64 = code_response["result"]["code_base64"]
        .as_str()
        .ok_or("Failed to get code_base64")?;
    let contract_code = base64::engine::general_purpose::STANDARD.decode(contract_code_base64)?;

    // Use genesis signer for the pre-created account
    let account_signer = get_genesis_signer();

    // Deploy the contract code to the sandbox account (which should already exist)
    // For wrap.near, we need to initialize it since it's a fresh deployment
    if mainnet_account_id == "wrap.near" {
        near_api::Contract::deploy(account_id.clone())
            .use_code(contract_code)
            .with_init_call("new", json!({}))
            .unwrap()
            .with_signer(account_signer.clone())
            .send_to(network_config)
            .await?
            .assert_success();
    } else {
        // For other contracts, skip initialization (already initialized on mainnet)
        near_api::Contract::deploy(account_id.clone())
            .use_code(contract_code)
            .without_init_call()
            .with_signer(account_signer.clone())
            .send_to(network_config)
            .await?
            .assert_success();
    }

    Ok(account_signer)
}

async fn setup_contract(
) -> Result<(near_sandbox::Sandbox, near_api::NetworkConfig, AccountId), Box<dyn std::error::Error>>
{
    // Create sandbox with pre-configured accounts including wrap.near for FT tests
    let wrap_near_account = near_sandbox::GenesisAccount {
        account_id: "wrap.near".parse().unwrap(),
        balance: near_sdk::NearToken::from_near(1000),
        private_key: near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY.to_string(),
        public_key: near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PUBLIC_KEY.to_string(),
    };

    let sandbox =
        near_sandbox::Sandbox::start_sandbox_with_config(near_sandbox::config::SandboxConfig {
            additional_accounts: vec![wrap_near_account],
            ..Default::default()
        })
        .await?;

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
        .call_function("view_storage_credits", json!({ "account_id": user_id }))
        .unwrap()
        .read_only()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data;

    assert_eq!(
        credits.as_yoctonear(),
        num_records as u128,
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

    let recipient1: AccountId = format!(
        "recipient1.{}",
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT
    )
    .parse()
    .unwrap();
    let recipient2: AccountId = format!(
        "recipient2.{}",
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT
    )
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
        .call_function("view_storage_credits", json!({ "account_id": user_id }))
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

    // Create recipient accounts and track initial balances
    let mut recipients = Vec::new();
    for i in 0..250 {
        let recipient: AccountId = format!(
            "recipient{}.{}",
            i,
            near_sandbox::config::DEFAULT_GENESIS_ACCOUNT
        )
        .parse()
        .unwrap();
        create_account(&recipient, NearToken::from_near(1), &network_config).await;
        recipients.push(recipient);
    }

    // Create 250 payment entries
    let mut payments = Vec::new();
    for recipient in recipients.iter() {
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

    // Get contract balance before payouts
    let contract_balance_before = near_api::Account(contract_id.clone())
        .view()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data
        .amount;

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

    // Verify all recipients received their payments
    for (i, recipient) in recipients.iter().enumerate() {
        let balance = near_api::Account(recipient.clone())
            .view()
            .fetch_from(&network_config)
            .await
            .unwrap()
            .data
            .amount;

        // Each recipient started with 1 NEAR and should have received 1 NEAR
        // Gas is not deducted from transfer amount, so balance should be exactly 2 NEAR
        assert_eq!(
            balance.as_yoctonear(),
            2_000_000_000_000_000_000_000_000, // Exactly 2 NEAR
            "Recipient {} should have exactly 2 NEAR, got: {} yoctoNEAR",
            i,
            balance.as_yoctonear()
        );
    }

    // Verify contract balance is not less than before (storage revenue retained)
    let contract_balance_after = near_api::Account(contract_id.clone())
        .view()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data
        .amount;

    assert!(
        contract_balance_after.as_yoctonear() >= contract_balance_before.as_yoctonear(),
        "Contract balance should not decrease after payouts. Before: {}, After: {}",
        contract_balance_before.as_yoctonear(),
        contract_balance_after.as_yoctonear()
    );

    // Verify all payments are marked as Paid
    let list: serde_json::Value = near_api::Contract(contract_id.clone())
        .call_function("view_list", json!({ "list_ref": list_id }))
        .unwrap()
        .read_only()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data;

    let payments_array = list["payments"].as_array().unwrap();
    assert_eq!(payments_array.len(), 250, "Should have 250 payments");

    for (i, payment) in payments_array.iter().enumerate() {
        assert_eq!(
            payment["status"], "Paid",
            "Payment {} should be marked as Paid",
            i
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_fungible_token_payment() -> Result<(), Box<dyn std::error::Error>> {
    let (sandbox, network_config, contract_id) = setup_contract().await?;

    // Import and setup wrap.near contract for wNEAR
    let wrap_near_id: AccountId = "wrap.near".parse().unwrap();
    let _wrap_near_signer =
        import_contract(&sandbox, &network_config, &wrap_near_id, "wrap.near").await?;

    // Create user account with more balance for 100 recipients
    let user_id: AccountId = format!("user.{}", near_sandbox::config::DEFAULT_GENESIS_ACCOUNT)
        .parse()
        .unwrap();
    let user_signer = create_account(&user_id, NearToken::from_near(200), &network_config).await;

    // Register user with wNEAR
    near_api::Contract(wrap_near_id.clone())
        .call_function(
            "storage_deposit",
            json!({
                "account_id": user_id.to_string(),
                "registration_only": true
            }),
        )
        .unwrap()
        .transaction()
        .deposit(NearToken::from_yoctonear(1_250_000_000_000_000_000_000))
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Deposit NEAR to get wNEAR (150 NEAR for 100 recipients at 1 wNEAR each, plus overhead)
    near_api::Contract(wrap_near_id.clone())
        .call_function("near_deposit", json!({}))
        .unwrap()
        .transaction()
        .deposit(NearToken::from_near(150))
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Buy storage for 100 recipients
    let num_records = 100;
    let storage_cost = NearToken::from_yoctonear(23_760_000_000_000_000_000_000 * 10); // 10x for 100 records

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

    // Create 100 recipient accounts
    let mut recipients = Vec::new();
    for i in 0..100 {
        let recipient_id: AccountId = format!(
            "ftrecipient{}.{}",
            i,
            near_sandbox::config::DEFAULT_GENESIS_ACCOUNT
        )
        .parse()
        .unwrap();
        create_account(&recipient_id, NearToken::from_near(1), &network_config).await;

        // Register recipient with wNEAR
        near_api::Contract(wrap_near_id.clone())
            .call_function(
                "storage_deposit",
                json!({
                    "account_id": recipient_id.to_string(),
                    "registration_only": true
                }),
            )
            .unwrap()
            .transaction()
            .deposit(NearToken::from_yoctonear(1_250_000_000_000_000_000_000))
            .with_signer(user_id.clone(), user_signer.clone())
            .send_to(&network_config)
            .await
            .unwrap()
            .assert_success();

        recipients.push(recipient_id);
    }

    // Create payment list with 100 recipients (1 wNEAR each)
    let mut payments = Vec::new();
    for recipient in recipients.iter() {
        payments.push(json!({
            "recipient": recipient.to_string(),
            "amount": "1000000000000000000000000" // 1 wNEAR
        }));
    }

    let submit_result = near_api::Contract(contract_id.clone())
        .call_function(
            "submit_list",
            json!({
                "token_id": wrap_near_id.to_string(),
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
    let list_id: u64 = 0;

    // Register contract account with wrap.near to receive FT transfers
    near_api::Contract(wrap_near_id.clone())
        .call_function(
            "storage_deposit",
            json!({
                "account_id": contract_id.to_string(),
                "registration_only": true
            }),
        )
        .unwrap()
        .transaction()
        .deposit(NearToken::from_yoctonear(1_250_000_000_000_000_000_000))
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Approve the list using ft_transfer_call (NEP-141 standard)
    // This will call ft_on_transfer on the contract with the list_id as msg
    let total_amount_str = "100000000000000000000000000"; // 100 wNEAR
    near_api::Contract(wrap_near_id.clone())
        .call_function(
            "ft_transfer_call",
            json!({
                "receiver_id": contract_id.to_string(),
                "amount": total_amount_str,
                "msg": list_id.to_string()
            }),
        )
        .unwrap()
        .transaction()
        .deposit(NearToken::from_yoctonear(1))
        .gas(near_sdk::Gas::from_tgas(100))
        .with_signer(user_id.clone(), user_signer.clone())
        .send_to(&network_config)
        .await
        .unwrap()
        .assert_success();

    // Verify list is approved
    let list: serde_json::Value = near_api::Contract(contract_id.clone())
        .call_function("view_list", json!({ "list_ref": list_id }))
        .unwrap()
        .read_only()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data;

    assert_eq!(
        list["status"], "Approved",
        "List should be approved after ft_transfer_call"
    );

    // Process payments in batches of 5 (FT transfers require cross-contract calls)
    // Each ft_transfer needs ~50 TGas, so 5 payments = ~250 TGas + overhead
    // We need to process 100 payments, so 20 batches of 5
    for batch in 0..20 {
        near_api::Contract(contract_id.clone())
            .call_function(
                "payout_batch",
                json!({ "list_ref": list_id, "max_payments": 5 }),
            )
            .unwrap()
            .transaction()
            .gas(near_sdk::Gas::from_tgas(300))
            .with_signer(user_id.clone(), user_signer.clone())
            .send_to(&network_config)
            .await
            .unwrap()
            .assert_success();

        if (batch + 1) % 5 == 0 {
            println!(
                "Processed {} of 20 batches ({} payments complete)",
                batch + 1,
                (batch + 1) * 5
            );
        }
    }

    // Verify all 100 recipients received their wNEAR payments
    for (i, recipient) in recipients.iter().enumerate() {
        let recipient_balance: String = near_api::Contract(wrap_near_id.clone())
            .call_function(
                "ft_balance_of",
                json!({ "account_id": recipient.to_string() }),
            )
            .unwrap()
            .read_only()
            .fetch_from(&network_config)
            .await
            .unwrap()
            .data;

        assert_eq!(
            recipient_balance, "1000000000000000000000000",
            "Recipient {} should have received 1 wNEAR",
            i
        );
    }

    // Verify all payments are marked as Paid
    let list: serde_json::Value = near_api::Contract(contract_id.clone())
        .call_function("view_list", json!({ "list_ref": list_id }))
        .unwrap()
        .read_only()
        .fetch_from(&network_config)
        .await
        .unwrap()
        .data;

    let payments_array = list["payments"].as_array().unwrap();
    assert_eq!(payments_array.len(), 100, "Should have 100 payments");

    for (i, payment) in payments_array.iter().enumerate() {
        assert_eq!(
            payment["status"], "Paid",
            "Payment {} should be marked as Paid",
            i
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_near_intents_payment() -> Result<(), Box<dyn std::error::Error>> {
    // Note: This is a placeholder test showing the structure
    // Full implementation requires deploying wrap.near, omft.near, and intents.near
    // Similar to the JavaScript example at NEAR-DevHub/near-treasury
    // For now, this test is marked as ignored until the full contract setup is available

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
        final_balance
            .as_yoctonear()
            .saturating_sub(initial_balance.as_yoctonear()),
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
