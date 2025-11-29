//! NEAR Sandbox Initializer for Treasury Test Environment
//!
//! This binary starts a NEAR sandbox, deploys the required contracts,
//! and keeps the sandbox running for testing.
//!
//! Based on the integration test patterns from tests/integration_tests.rs
//! and examples from https://github.com/near/near-sandbox-rs

use anyhow::{Context, Result};
use base64::Engine;
use near_api::{AccountId, NetworkConfig, NearToken, Signer};
use near_sandbox::{GenesisAccount, Sandbox, SandboxConfig};
use std::sync::Arc;
use tracing::{error, info, warn};

/// Get the genesis signer for the sandbox
fn get_genesis_signer() -> Arc<Signer> {
    let genesis_account = GenesisAccount::default();
    Signer::new(Signer::from_secret_key(
        genesis_account.private_key.parse().unwrap(),
    ))
    .unwrap()
}

/// Create a new account with the given balance
async fn create_account(
    new_account_id: &AccountId,
    balance: NearToken,
    network_config: &NetworkConfig,
) -> Result<Arc<Signer>> {
    info!("Creating account: {}", new_account_id);

    let genesis_account = GenesisAccount::default();

    near_api::Account::create_account(new_account_id.clone())
        .fund_myself(
            new_account_id.get_parent_account_id().unwrap().to_owned(),
            balance,
        )
        .public_key(
            genesis_account.public_key
                .parse::<near_api::PublicKey>()
                .unwrap(),
        )
        .unwrap()
        .with_signer(get_genesis_signer())
        .send_to(network_config)
        .await
        .context(format!("Failed to create account {}", new_account_id))?
        .assert_success();

    Ok(get_genesis_signer())
}

/// Import a contract from mainnet to the sandbox
async fn import_contract(
    network_config: &NetworkConfig,
    account_id: &AccountId,
    mainnet_account_id: &str,
) -> Result<()> {
    info!(
        "Importing contract {} from mainnet as {}",
        mainnet_account_id, account_id
    );

    // Configure mainnet connection
    let mainnet_config = NetworkConfig::mainnet();

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
        .await
        .context("Failed to fetch contract code from mainnet")?
        .json()
        .await
        .context("Failed to parse mainnet response")?;

    let contract_code_base64 = code_response["result"]["code_base64"]
        .as_str()
        .context("Failed to get code_base64 from response")?;
    let contract_code = base64::engine::general_purpose::STANDARD
        .decode(contract_code_base64)
        .context("Failed to decode contract code")?;

    info!(
        "Fetched {} bytes of contract code for {}",
        contract_code.len(),
        mainnet_account_id
    );

    // Use genesis signer for the pre-created account
    let account_signer = get_genesis_signer();

    // Deploy the contract code to the sandbox account
    // For wrap.near, we need to initialize it since it's a fresh deployment
    if mainnet_account_id == "wrap.near" {
        near_api::Contract::deploy(account_id.clone())
            .use_code(contract_code)
            .with_init_call("new", serde_json::json!({}))
            .unwrap()
            .with_signer(account_signer.clone())
            .send_to(network_config)
            .await
            .context(format!("Failed to deploy {} with init", account_id))?
            .assert_success();
    } else {
        // For other contracts, skip initialization (already initialized on mainnet)
        near_api::Contract::deploy(account_id.clone())
            .use_code(contract_code)
            .without_init_call()
            .with_signer(account_signer.clone())
            .send_to(network_config)
            .await
            .context(format!("Failed to deploy {}", account_id))?
            .assert_success();
    }

    info!("Successfully deployed contract to {}", account_id);
    Ok(())
}

/// Deploy the bulk payment contract from a WASM file
async fn deploy_bulk_payment_contract(
    network_config: &NetworkConfig,
    contract_id: &AccountId,
    wasm_path: &str,
) -> Result<()> {
    info!("Deploying bulk payment contract to {}", contract_id);

    let contract_code = std::fs::read(wasm_path)
        .context(format!("Failed to read contract WASM from {}", wasm_path))?;

    info!("Read {} bytes of bulk payment contract", contract_code.len());

    let contract_signer = get_genesis_signer();

    near_api::Contract::deploy(contract_id.clone())
        .use_code(contract_code)
        .with_init_call("new", ())
        .unwrap()
        .with_signer(contract_signer)
        .send_to(network_config)
        .await
        .context("Failed to deploy bulk payment contract")?
        .assert_success();

    info!("Successfully deployed bulk payment contract to {}", contract_id);
    Ok(())
}

/// Deploy a DAO contract from a WASM file
async fn deploy_dao_contract(
    network_config: &NetworkConfig,
    contract_id: &AccountId,
    wasm_path: &str,
) -> Result<()> {
    info!("Deploying DAO contract to {}", contract_id);

    let contract_code = std::fs::read(wasm_path)
        .context(format!("Failed to read DAO WASM from {}", wasm_path))?;

    info!("Read {} bytes of DAO contract", contract_code.len());

    let contract_signer = get_genesis_signer();

    let genesis_account = GenesisAccount::default();

    // Initialize DAO with a simple policy
    let policy = serde_json::json!({
        "roles": [{
            "name": "council",
            "kind": { "Group": [genesis_account.account_id.to_string()] },
            "permissions": ["*:*"],
            "vote_policy": {}
        }],
        "default_vote_policy": {
            "weight_kind": "RoleWeight",
            "quorum": "0",
            "threshold": [1, 2]
        },
        "proposal_bond": "100000000000000000000000",
        "proposal_period": "604800000000000",
        "bounty_bond": "100000000000000000000000",
        "bounty_forgiveness_period": "604800000000000"
    });

    let init_args = serde_json::json!({
        "config": {
            "name": "Sample DAO",
            "purpose": "Testing DAO for sandbox",
            "metadata": ""
        },
        "policy": policy
    });

    near_api::Contract::deploy(contract_id.clone())
        .use_code(contract_code)
        .with_init_call("new", init_args)
        .unwrap()
        .with_signer(contract_signer)
        .send_to(network_config)
        .await
        .context("Failed to deploy DAO contract")?
        .assert_success();

    info!("Successfully deployed DAO contract to {}", contract_id);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("sandbox_init=info".parse().unwrap())
                .add_directive("near_sandbox=info".parse().unwrap()),
        )
        .init();

    info!("Starting NEAR Treasury Sandbox Environment");

    // Get configuration from environment
    let contracts_dir =
        std::env::var("CONTRACTS_DIR").unwrap_or_else(|_| "/app/contracts".to_string());

    // Get the default genesis account credentials
    let genesis_account = GenesisAccount::default();

    // Create genesis accounts for top-level accounts we need
    let wrap_near_account = GenesisAccount {
        account_id: "wrap.near".parse().unwrap(),
        balance: NearToken::from_near(1000),
        private_key: genesis_account.private_key.clone(),
        public_key: genesis_account.public_key.clone(),
    };

    let intents_account = GenesisAccount {
        account_id: "intents.near".parse().unwrap(),
        balance: NearToken::from_near(1000),
        private_key: genesis_account.private_key.clone(),
        public_key: genesis_account.public_key.clone(),
    };

    let omft_account = GenesisAccount {
        account_id: "omft.near".parse().unwrap(),
        balance: NearToken::from_near(1000),
        private_key: genesis_account.private_key.clone(),
        public_key: genesis_account.public_key.clone(),
    };

    info!("Starting NEAR sandbox...");

    // Configure sandbox to run on port 3031 internally
    // A socat proxy will forward 0.0.0.0:3030 -> 127.0.0.1:3031 for external access
    let config = SandboxConfig {
        rpc_port: Some(3031),
        additional_accounts: vec![wrap_near_account, intents_account, omft_account],
        ..Default::default()
    };

    // Start sandbox with pre-configured accounts
    let sandbox = Sandbox::start_sandbox_with_config(config)
        .await
        .context("Failed to start sandbox")?;

    info!("Sandbox started at RPC address: {}", sandbox.rpc_addr);
    
    // The sandbox binds to 127.0.0.1 internally, which is accessible within the container.
    // For external Docker access, we need to use the host network or rely on Docker port forwarding
    // to map 0.0.0.0:3030 on the host to 127.0.0.1:3030 inside the container.

    // Configure network using the sandbox RPC address
    let network_config = NetworkConfig::from_rpc_url("sandbox", sandbox.rpc_addr.parse().unwrap());

    info!("================================================");
    info!("Initializing contracts...");
    info!("================================================");

    // Import contracts from mainnet
    if let Err(e) = import_contract(&network_config, &"wrap.near".parse().unwrap(), "wrap.near").await
    {
        error!("Failed to import wrap.near: {}", e);
    }

    if let Err(e) =
        import_contract(&network_config, &"intents.near".parse().unwrap(), "intents.near").await
    {
        error!("Failed to import intents.near: {}", e);
    }

    if let Err(e) = import_contract(&network_config, &"omft.near".parse().unwrap(), "omft.near").await
    {
        error!("Failed to import omft.near: {}", e);
    }

    // Create sub-accounts for bulk payment contract
    let bulk_payment_id: AccountId = format!(
        "bulk-payment.{}",
        genesis_account.account_id
    )
    .parse()
    .unwrap();

    if let Err(e) = create_account(&bulk_payment_id, NearToken::from_near(100), &network_config).await
    {
        error!("Failed to create bulk-payment account: {}", e);
    }

    // Deploy bulk payment contract if available
    let bulk_payment_wasm = format!("{}/bulk_payment.wasm", contracts_dir);
    if std::path::Path::new(&bulk_payment_wasm).exists() {
        if let Err(e) =
            deploy_bulk_payment_contract(&network_config, &bulk_payment_id, &bulk_payment_wasm).await
        {
            error!("Failed to deploy bulk payment contract: {}", e);
        }
    } else {
        warn!(
            "Bulk payment contract not found at {}, skipping deployment",
            bulk_payment_wasm
        );
    }

    // Deploy sample DAO if available
    let dao_wasm = format!("{}/sputnikdao2.wasm", contracts_dir);
    if std::path::Path::new(&dao_wasm).exists() {
        // Create DAO account
        let dao_id: AccountId = format!("sample-dao.{}", genesis_account.account_id)
            .parse()
            .unwrap();

        if let Err(e) = create_account(&dao_id, NearToken::from_near(100), &network_config).await {
            error!("Failed to create DAO account: {}", e);
        } else if let Err(e) = deploy_dao_contract(&network_config, &dao_id, &dao_wasm).await {
            error!("Failed to deploy DAO contract: {}", e);
        }
    } else {
        info!("DAO contract not found at {}, skipping", dao_wasm);
    }

    info!("================================================");
    info!("Sandbox initialization complete!");
    info!("================================================");
    info!("");
    info!("RPC URL: {}", sandbox.rpc_addr);
    info!("Available contracts:");
    info!("  - wrap.near");
    info!("  - intents.near");
    info!("  - omft.near");
    info!("  - {}", bulk_payment_id);
    info!("");

    // Keep the sandbox running
    info!("Sandbox is running. Press Ctrl+C to stop.");

    // Wait indefinitely (sandbox process will be killed when container stops)
    tokio::signal::ctrl_c()
        .await
        .context("Failed to wait for Ctrl+C")?;

    info!("Shutting down sandbox...");
    Ok(())
}
