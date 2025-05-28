// Common Deps
use base64::{Engine as _, engine::general_purpose};
use reqwest::{Client, Error};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use tokio::time::{Duration, sleep};

// Cosmos Deps
use cosmrs::{
    Coin, Denom,
    crypto::secp256k1::SigningKey,
    tx::{self, Fee, SignDoc, SignerInfo},
};
use osmosis_std::types::osmosis::{
    gamm::v1beta1::MsgSwapExactAmountIn, poolmanager::v1beta1::SwapAmountInRoute,
};
use prost::Message;
use std::error::Error as StdError;

// Solana Deps
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_request::TokenAccountsFilter;
use solana_sdk::{
    native_token::lamports_to_sol,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};

use spl_associated_token_account::get_associated_token_address;

#[derive(Debug, Clone, Deserialize)]
pub struct PoolAsset {
    pub token: Token,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Token {
    pub denom: String,
    pub amount: String,
}

#[derive(Debug, Deserialize)]
struct Pool {
    pool_assets: Vec<PoolAsset>,
}

#[derive(Debug, Deserialize)]
struct PoolResponse {
    pool: Pool,
}

#[derive(Debug, Deserialize)]
struct Balance {
    denom: String,
    amount: String,
}

#[derive(Debug, Deserialize)]
struct BalancesResponse {
    balances: Vec<Balance>,
}

#[derive(Debug, Deserialize)]
pub struct BaseAccount {
    pub account_number: String,
    pub sequence: String,
}

#[derive(Debug, Deserialize)]
pub struct AccountWrapper {
    pub account: BaseAccount,
}

// COSMOS UTILS START
pub async fn get_pool_assets(
    pool_id: &str,
) -> Result<(PoolAsset, PoolAsset), Box<dyn std::error::Error>> {
    let url = format!(
        "https://osmosis-api.polkachu.com/osmosis/gamm/v1beta1/pools/{}",
        pool_id
    );

    let res = reqwest::get(&url).await?.json::<PoolResponse>().await?;

    if res.pool.pool_assets.len() == 2 {
        let token_a = res.pool.pool_assets[0].clone();
        let token_b = res.pool.pool_assets[1].clone();

        Ok((token_a, token_b))
    } else {
        Err("Pool doesn't have exactly 2 assets".into())
    }
}

pub async fn get_token_price_by_pool(pool_id: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let (token_a, token_b) = get_pool_assets(pool_id).await?;

    let amount_a: f64 = token_a.token.amount.parse()?;
    let amount_b: f64 = token_b.token.amount.parse()?;

    let price = amount_b / amount_a;

    println!(
        "1 {} ≈ {} {}",
        token_a.token.denom, price, token_b.token.denom
    );

    Ok(price)
}

pub async fn get_wallet_balance(address: &str) -> Result<HashMap<String, f64>, Error> {
    let url = format!(
        "https://osmosis-api.polkachu.com/cosmos/bank/v1beta1/balances/{}",
        address
    );

    let res = reqwest::get(&url).await?.json::<BalancesResponse>().await?;

    let mut balance_map = HashMap::new();

    for balance in res.balances {
        let amount: f64 = balance.amount.parse().unwrap_or(0.0);

        let key = if balance.denom.contains('/') {
            balance
                .denom
                .split('/')
                .last()
                .unwrap_or(&balance.denom)
                .to_string()
        } else {
            balance.denom.clone()
        };

        balance_map.insert(key, amount);
    }

    Ok(balance_map)
}

pub async fn get_token_balance(
    address: &str,
    denom: &str,
) -> Result<f64, Box<dyn std::error::Error>> {
    let balances = get_wallet_balance(address).await?;

    // Try exact match
    if let Some(balance) = balances.get(denom) {
        return Ok(*balance);
    }

    // Try without "ibc/" prefix if present
    if denom.starts_with("ibc/") {
        let raw_hash = &denom[4..]; // removes "ibc/"
        if let Some(balance) = balances.get(raw_hash) {
            return Ok(*balance);
        }
    }

    // Try suffix match
    for (key, value) in &balances {
        if key.ends_with(denom) {
            println!("Matched by suffix: {} => {}", key, denom);
            return Ok(*value);
        }
    }

    println!("Token '{}' not found. Available tokens:", denom);
    for k in balances.keys() {
        println!("  - {}", k);
    }

    Err(format!("Token {} not found in wallet", denom).into())
}

pub async fn pool_swap(
    address: &str,
    pool_id: &str,
    input_token_denom: &str,
    output_token_denom: &str,
    amount_in: f64,
    slippage_tolerance: f64,
) -> Result<(u64, u64, cosmrs::Any), Box<dyn std::error::Error>> {
    // === Fetch account_number and sequence ===
    let client = Client::new();
    let url = format!(
        "https://osmosis-api.polkachu.com/cosmos/auth/v1beta1/accounts/{}",
        address
    );
    let res = client.get(&url).send().await?;
    let account_data: AccountWrapper = res.json().await?;

    let account_number = account_data.account.account_number.parse::<u64>()?;
    let sequence = account_data.account.sequence.parse::<u64>()?;

    // === Get price and simulate output ===
    let (asset_a, asset_b) = get_pool_assets(pool_id).await?;
    let amount_a: f64 = asset_a.token.amount.parse()?;
    let amount_b: f64 = asset_b.token.amount.parse()?;

    // Detect which is input/output
    let (reserve_in, reserve_out) = if asset_a.token.denom == input_token_denom {
        (amount_a, amount_b)
    } else {
        (amount_b, amount_a)
    };

    let estimated_out = simulate_swap_math(amount_in, reserve_in, reserve_out, 0.003);
    let min_out = estimated_out * (1.0 - slippage_tolerance);

    // === Print the math ===
    println!("\n📊 Swap Preview:");
    println!(
        "  Input:             {:.6} {}",
        amount_in, input_token_denom
    );
    println!(
        "  Estimated Output:  {:.6} {}",
        estimated_out, output_token_denom
    );
    println!("  Slippage Tolerance: {:.2}%", slippage_tolerance * 100.0);
    println!(
        "  Min Output (set in tx): {:.6} {}\n",
        min_out, output_token_denom
    );

    // === Build MsgSwapExactAmountIn ===
    let token_in_amount = ((amount_in * 1_000_000.0).round()) as u64;
    let token_out_min_amount = (min_out * 1_000_000.0).round() as u64;

    let msg = MsgSwapExactAmountIn {
        sender: address.to_string().clone(),
        routes: vec![SwapAmountInRoute {
            pool_id: pool_id.parse::<u64>()?,
            token_out_denom: output_token_denom.to_string(),
        }],
        token_in: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: input_token_denom.to_string(),
            amount: token_in_amount.to_string(),
        }),
        token_out_min_amount: token_out_min_amount.to_string(),
    };

    let msg_any = cosmrs::Any {
        type_url: "/osmosis.gamm.v1beta1.MsgSwapExactAmountIn".to_string(),
        value: msg.encode_to_vec(),
    };
    Ok((account_number, sequence, msg_any))
}

pub async fn sign_tx_broadcast(
    msg_any: cosmrs::Any,
    public_key: cosmrs::crypto::PublicKey,
    sequence: u64,
    account_number: u64,
    signing_key: &SigningKey,
) -> Result<String, Box<dyn std::error::Error>> {
    // === Construct Tx body ===
    let tx_body = tx::Body::new(vec![msg_any], "", 0u32);

    let fee = Coin {
        denom: Denom::from_str("uosmo").unwrap(),
        amount: 1000,
    };

    let gas_limit: u64 = 200_000;
    let signer_info = SignerInfo::single_direct(Some(public_key), sequence);
    let fee = Fee::from_amount_and_gas(fee, gas_limit);
    let auth_info = tx::AuthInfo {
        signer_infos: vec![signer_info],
        fee,
    };

    let sign_doc = SignDoc::new(
        &tx_body,
        &auth_info,
        &cosmrs::tendermint::chain::Id::from_str("osmosis-1").unwrap(),
        account_number,
    )?;
    let tx_raw = sign_doc.sign(&signing_key)?;

    // === Broadcast transaction ===
    let tx_bytes = tx_raw.to_bytes()?;
    let base64_tx = general_purpose::STANDARD.encode(tx_bytes);
    println!("Broadcast result: {}", base64_tx);

    let res = reqwest::Client::new()
        .post("https://osmosis-api.polkachu.com/cosmos/tx/v1beta1/txs")
        .json(&serde_json::json!({
            "tx_bytes": base64_tx,
            "mode": "BROADCAST_MODE_SYNC"
        }))
        .send()
        .await?;

    let response_json: serde_json::Value = res.json().await?;
    let txhash = response_json["tx_response"]["txhash"]
        .as_str()
        .ok_or("Missing txhash in response")?
        .to_string();

    println!("✅ Broadcast txhash: {}", txhash);
    Ok(txhash)
}

pub async fn check_tx_success(
    txhash: &str,
) -> Result<Option<(bool, Option<String>, Option<(f64, f64)>)>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://osmosis-api.polkachu.com/cosmos/tx/v1beta1/txs/{}",
        txhash
    );

    let res = reqwest::Client::new().get(&url).send().await?;

    if !res.status().is_success() {
        let json: Value = res.json().await.unwrap_or_default();
        let message = json["message"]
            .as_str()
            .unwrap_or("Transaction not found or not indexed yet");
        println!("❓ Tx not found: {}", message);
        return Ok(None);
    }

    let json: Value = res.json().await?;

    let code = json["tx_response"]["code"].as_u64().unwrap_or(0);
    let raw_log = json["tx_response"]["raw_log"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let mut tokens_in: Option<f64> = None;
    let mut tokens_out: Option<f64> = None;

    if let Some(events) = json["tx_response"]["events"].as_array() {
        for event in events {
            if event["type"] == "token_swapped" {
                let vec_a = vec![];
                let attrs = event["attributes"].as_array().unwrap_or(&vec_a);
                for attr in attrs {
                    if attr["key"] == "tokens_in" {
                        let val = attr["value"].as_str().unwrap_or_default();
                        if let Some(num) = val.split("uosmo").next() {
                            tokens_in = Some(num.parse::<f64>().unwrap_or(0.0) / 1_000_000.0);
                        }
                    } else if attr["key"] == "tokens_out" {
                        let val = attr["value"].as_str().unwrap_or_default();
                        if let Some(num) = val.split("ibc/").next() {
                            tokens_out = Some(num.parse::<f64>().unwrap_or(0.0) / 1_000_000.0);
                        }
                    }
                }
            }
        }
    }

    if code == 0 {
        println!("✅ Tx {} successful", txhash);
        return Ok(Some((
            true,
            None,
            Some((tokens_in.unwrap_or(0.0), tokens_out.unwrap_or(0.0))),
        )));
    } else {
        println!("❌ Tx {} failed with code {}: {}", txhash, code, raw_log);
        return Ok(Some((false, Some(raw_log), None)));
    }
}

pub async fn wait_for_tx_confirmation(
    txhash: &str,
    max_attempts: u32,
    delay_secs: u64,
) -> Result<(bool, Option<String>, Option<(f64, f64)>), Box<dyn std::error::Error>> {
    for attempt in 1..=max_attempts {
        println!(
            "⏳ Checking tx ({}) attempt {}/{}...",
            txhash, attempt, max_attempts
        );

        match check_tx_success(txhash).await? {
            Some((true, _, Some(amounts))) => return Ok((true, None, Some(amounts))),
            Some((true, _, None)) => return Ok((true, None, None)), // <-- Handle success without amounts
            Some((false, Some(log), _)) => return Ok((false, Some(log), None)),
            Some((false, None, _)) => return Ok((false, Some("Unknown failure".into()), None)),
            None => {
                sleep(Duration::from_secs(delay_secs)).await;
            }
        }
    }

    println!("❌ Timed out waiting for tx confirmation.");
    Ok((
        false,
        Some("Timeout: tx not confirmed within expected time".into()),
        None,
    ))
}

pub fn simulate_swap_math(amount_in: f64, reserve_in: f64, reserve_out: f64, fee: f64) -> f64 {
    let dx = amount_in * 1_000_000.0; // to base units
    let fee_factor = 1.0 - fee;
    let numerator = reserve_out * dx * fee_factor;
    let denominator = reserve_in * 1.0 + dx * fee_factor;
    (numerator / denominator) / 1_000_000.0
}

// This is the way back (Osmosis Complexity Layer)
pub async fn simulate_swap_via_lcd(
    msg_any: cosmrs::Any,
    public_key: cosmrs::crypto::PublicKey,
    sequence: u64,
    account_number: u64,
    signing_key: &SigningKey,
) -> Result<f64, Box<dyn std::error::Error>> {
    let tx_body = tx::Body::new(vec![msg_any.clone()], "", 0u32);

    let fee = Coin {
        denom: Denom::from_str("uosmo")?,
        amount: 3000,
    };

    let gas_limit: u64 = 200_000;
    let signer_info = SignerInfo::single_direct(Some(public_key), sequence);
    let auth_info = tx::AuthInfo {
        signer_infos: vec![signer_info],
        fee: Fee::from_amount_and_gas(fee.clone(), gas_limit),
    };

    let sign_doc = SignDoc::new(
        &tx_body,
        &auth_info,
        &cosmrs::tendermint::chain::Id::from_str("osmosis-1")?,
        account_number,
    )?;
    let tx_raw = sign_doc.sign(&signing_key)?;

    // === Simulate endpoint ===
    let tx_bytes = tx_raw.to_bytes()?;
    let base64_tx = general_purpose::STANDARD.encode(tx_bytes);

    let simulate_url = "https://osmosis-api.polkachu.com/cosmos/tx/v1beta1/simulate";

    let res = reqwest::Client::new()
        .post(simulate_url)
        .json(&serde_json::json!({ "tx_bytes": base64_tx }))
        .send()
        .await?;

    let json: serde_json::Value = res.json().await?;

    // Extract logs from simulation preview
    if let Some(logs) = json["result"]["events"].as_array() {
        for event in logs {
            if event["type"] == "token_swapped" {
                for attr in event["attributes"].as_array().unwrap_or(&vec![]) {
                    if attr["key"] == "tokens_out" {
                        let value = attr["value"].as_str().unwrap_or("");
                        if value.contains("uosmo") {
                            let amount_str = value.replace("uosmo", "");
                            let amount = amount_str.parse::<f64>()? / 1_000_000.0;
                            return Ok(amount);
                        }
                    }
                }
            }
        }
    }

    Err("No swap result in simulation response".into())
}

// SOLANA UTILS START

pub async fn sol_get_sol_balance(
    rpc_url: &str,
    wallet_pubkey: &Pubkey,
) -> Result<f64, Box<dyn std::error::Error>> {
    let client = RpcClient::new(rpc_url.to_string());

    let lamports = client.get_balance(wallet_pubkey).await?;
    println!("🪙 Raw lamports: {}", lamports);

    let sol = lamports_to_sol(lamports);
    Ok(sol)
}

pub async fn get_usdc_balance(wallet_address: &str, usdc_mint: &str) -> f64 {
    // USDC Mint on Solana mainnet
    let rpc_url = "https://api.mainnet-beta.solana.com";

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTokenAccountsByOwner",
        "params": [
            wallet_address,
            { "mint": usdc_mint },
            { "encoding": "jsonParsed" }
        ]
    });

    let client = Client::new();
    let res = client.post(rpc_url).json(&body).send().await.unwrap();

    let json: serde_json::Value = res.json().await.unwrap();

    let accounts = json["result"]["value"].as_array().ok_or("No accounts found").unwrap();

    let mut total_usdc = 0.0;
    for account in accounts {
        if let Some(amount_str) = account["account"]["data"]["parsed"]["info"]["tokenAmount"]["uiAmount"]
            .as_f64()
        {
            total_usdc += amount_str;
        }
    }

    total_usdc
}

pub async fn jupiter_swap(
    rpc_url: &str,
    input_mint: &str,
    output_mint: &str,
    amount: f64,
    slippage_bps: u64,
    user_keypair: &Keypair,
) -> Result<(f64, String), Box<dyn StdError>> {
    let user_pubkey = user_keypair.pubkey();
    let client = Client::new();

    // Convert amount based on input token decimals
    let input_decimals = if input_mint == "So11111111111111111111111111111111111111112" {
        9 // SOL
    } else {
        6 // USDC or others
    };
    let amount_in_ui_units = token_amount_to_ui_units(amount, input_decimals);

    // === 1. Fetch quote
    let quote_url = format!(
        "https://quote-api.jup.ag/v6/quote?inputMint={}&outputMint={}&amount={}&slippageBps={}",
        input_mint, output_mint, amount_in_ui_units, slippage_bps
    );

    let quote_resp = client.get(&quote_url).send().await?.error_for_status()?;
    let quote_json: serde_json::Value = quote_resp.json().await?;
    println!("💸 Expected output: {}", quote_json["outAmount"]);

    // Parse raw output amount (in minor units)
    let out_amount_raw = quote_json["outAmount"]
        .as_str()
        .ok_or("Missing outAmount")?
        .parse::<f64>()?;

    // Convert based on output token decimals
    let output_decimals = if output_mint == "So11111111111111111111111111111111111111112" {
        9 // SOL
    } else {
        6 // USDC or others
    };
    let out_amount = out_amount_raw / 10f64.powi(output_decimals as i32);

    // === 2. Build swap request using full quote
    let swap_body = serde_json::json!({
        "quoteResponse": quote_json,
        "userPublicKey": user_pubkey.to_string(),
        "wrapUnwrapSOL": true
    });

    println!("🔍 Sending swap body: {}", swap_body);

    // === 3. Call Jupiter swap API
    let swap_resp = client
        .post("https://quote-api.jup.ag/v6/swap")
        .json(&swap_body)
        .send()
        .await?
        .error_for_status()?;

    let swap_json: serde_json::Value = swap_resp.json().await?;
    let tx_base64 = swap_json["swapTransaction"]
        .as_str()
        .ok_or("Missing swapTransaction field")?;

    // === 4. Decode, sign, and send transaction
    let tx_bytes = base64::decode(tx_base64)?;
    let mut tx: VersionedTransaction = bincode::deserialize(&tx_bytes)?;
    let sig = user_keypair.sign_message(&tx.message.serialize());
    tx.signatures[0] = sig;

    let rpc = RpcClient::new(rpc_url.to_string());
    let tx_signature = rpc.send_and_confirm_transaction(&tx).await?;

    println!("✅ Swap submitted! Signature: {}", tx_signature);

    // Return out_amount in proper units (SOL or USDC), and tx signature
    Ok((out_amount, tx_signature.to_string()))
}

fn token_amount_to_ui_units(amount: f64, decimals: u8) -> u64 {
    (amount * 10_f64.powi(decimals as i32)) as u64
}

pub async fn run_jupiter_bot(trading_flag: std::sync::Arc<tokio::sync::Mutex<bool>>) {
    let left_asset = "So11111111111111111111111111111111111111112";
    let right_asset = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    let sell_percentage: f64 = 2.3;
    let dca_recover_percentage: f64 = 3.5;
    let r_factor: f64 = 0.5;

    println!("Starting Jupiter Bot [{left_asset}/{right_asset}] ...");
    println!("Running strategy with parameters:");
    println!("- sell_percentage: {}%", sell_percentage);
    println!("- dca_recover_percentage: {}%", dca_recover_percentage);
    println!("- r_factor: {}%", r_factor);

    crate::jupiter_strategy_start::jup_bot_start(
        left_asset,
        right_asset,
        sell_percentage,
        dca_recover_percentage,
        r_factor,
        trading_flag
    )
    .await;
}