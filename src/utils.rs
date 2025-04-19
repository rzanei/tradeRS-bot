use reqwest;
use reqwest::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use serde_json::Value;

use base64::{Engine as _, engine::general_purpose};
use bip39::Mnemonic;
use cosmrs::{
    Coin, Denom,
    crypto::secp256k1::SigningKey,
    tx::{self, Fee, SignDoc, SignerInfo},
};
use osmosis_std::types::osmosis::{
    gamm::v1beta1::MsgSwapExactAmountIn, poolmanager::v1beta1::SwapAmountInRoute,
};
use prost::Message;
use reqwest::Client;
use std::str::FromStr;

#[derive(Debug, Clone, Deserialize)]
pub struct PoolAsset {
    pub token: Token,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Token {
    pub denom: String,
    amount: String,
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
    pub address: String,
    pub pub_key: Option<serde_json::Value>,
    pub account_number: String,
    pub sequence: String,
}

#[derive(Debug, Deserialize)]
pub struct AccountWrapper {
    pub account: BaseAccount,
}

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
    

    // === Build MsgSwapExactAmountIn ===
    let token_in_amount = ((amount_in * 1_000_000.0).round()) as u64;
    let token_out_min_amount =
        ((amount_in * (1.0 - slippage_tolerance)) * 1_000_000.0).round() as u64;

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
        amount: 3000,
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
) -> Result<Option<(bool, Option<String>)>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://osmosis-api.polkachu.com/cosmos/tx/v1beta1/txs/{}",
        txhash
    );

    let res = reqwest::Client::new()
        .get(&url)
        .send()
        .await?;

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

    if code == 0 {
        println!("✅ Tx {} successful", txhash);
        return Ok(Some((true, None)));
    } else {
        println!("❌ Tx {} failed with code {}: {}", txhash, code, raw_log);
        return Ok(Some((false, Some(raw_log))));
    }
}
use tokio::time::{sleep, Duration};

pub async fn wait_for_tx_confirmation(
    txhash: &str,
    max_attempts: u32,
    delay_secs: u64,
) -> Result<(bool, Option<String>), Box<dyn std::error::Error>> {
    for attempt in 1..=max_attempts {
        println!("⏳ Checking tx ({}) attempt {}/{}...", txhash, attempt, max_attempts);

        match check_tx_success(txhash).await? {
            Some((true, _)) => return Ok((true, None)),
            Some((false, Some(log))) => return Ok((false, Some(log))),
            Some((false, None)) => return Ok((false, Some("Unknown failure".into()))),
            None => {
                // Tx not found yet — wait and try again
                sleep(Duration::from_secs(delay_secs)).await;
            }
        }
    }

    println!("❌ Timed out waiting for tx confirmation.");
    Ok((false, Some("Timeout: tx not confirmed within expected time".into())))
}
