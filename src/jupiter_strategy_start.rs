use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer};
use chrono::{DateTime, Utc};

use crate::{
    log_manager::{load_trade_log, log_trade, read_log, write_log},
    trading_math::{decreased_amount_by_percentage, increased_amount_by_percentage},
    utils::{
        jupiter_swap, sol_get_sol_balance
    },
};
use std::{env, str::FromStr};

pub async fn jup_bot_start(
    left_asset: &str,
    right_asset: &str,
    buy_amount: f64,
    sell_percentage: f64,
    buy_percentage: f64,
    recover_percentage: f64,
) {
    dotenvy::from_path(".env").expect("Failed to load .env");
    let wallet_pk = env::var("SOL_WALLET_PK").expect("SOL_WALLET_PK not set in .env");
    let rpc_url = "https://api.mainnet-beta.solana.com";
    
    
    // === 1. Parse mnemonic, derive key and address ===
    let sol_keypair = Keypair::from_base58_string(&wallet_pk);
    let wallet_pubkey = sol_keypair.pubkey();
    println!("✅ Connected Wallet Address: {:?}", wallet_pubkey);

    let mut trade_log = load_trade_log(&format!("logs/pair_{left_asset}_{right_asset}_trade_history.json")).unwrap();
    println!("{:?}", trade_log);

    loop {
        let value = read_log(&format!("logs/pair_{left_asset}_{right_asset}_value.txt")).unwrap();
        let cooldown_secs = 3600; // 1 hour
        let now = Utc::now();
        match value.eq(&0.0) {
            true => {
            
                println!("🕒 Checking cooldown...");

                if let Some(last_trade) = trade_log.first() {
                    if last_trade.trade_type == "sell" {
                        let last_time = DateTime::parse_from_rfc3339(&last_trade.time)
                            .unwrap()
                            .with_timezone(&Utc);
                        let elapsed = now.signed_duration_since(last_time).num_seconds();
        
                        if elapsed < cooldown_secs {
                            println!(
                                "⏳ Cooldown active ({}s left). Skipping buy...",
                                cooldown_secs - elapsed
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                            continue;
                        }
                    }
                }

                println!("Make First Trade");

                let asset_b_balance = sol_get_sol_balance(
                    rpc_url,
                    &wallet_pubkey,
                )
                .await
                .unwrap();
                println!("Account Balance: {:?} SOL", asset_b_balance);
                let mut retries = 200;
                let mut slippage_bps = 1;
                let slippage_bps_max = 5;
                
                while retries > 0 {
                    println!("💵 Attempting to buy {:.6} worth of {} with slippage {}bps", buy_amount, left_asset, slippage_bps);
                
                    match jupiter_swap(
                        rpc_url,
                        right_asset, // USDC - what you have
                        left_asset,  // SOL - what you want to buy
                        buy_amount,
                        slippage_bps,
                        &sol_keypair,
                    ).await {
                        Ok((received_amount, tx_signature)) => {
                            println!("🎉 Buy successful! Received {:.6} {} in tx {}", received_amount, right_asset, tx_signature);
                
                            log_trade(
                                &format!("logs/pair_{}_{}_trade_history.json", left_asset, right_asset),
                                &mut trade_log,
                                "buy",
                                buy_amount,       // Correct: USDC spent
                                received_amount,  // Correct: SOL received
                            ).unwrap();
                
                            write_log(
                                &format!("logs/pair_{}_{}_value.txt", left_asset, right_asset),
                                &received_amount.to_string(),
                            ).unwrap();
                
                            break;
                        }
                        Err(e) => {
                            println!("⚠️ Swap attempt failed: {:?}", e);
                
                            retries -= 1;
                            if slippage_bps < slippage_bps_max {
                                slippage_bps += 1;
                            }
                
                            if retries > 0 {
                                println!("🔁 Retrying in 2 seconds... ({} retries left)", retries);
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            } else {
                                println!("❌ Max retries reached. Aborting swap.");
                                break;
                            }
                        }
                    }
                }
            }
            false => {
                println!("📈 Checking SELL conditions...");
            
                // 1. Get the last buy trade info
                let last_trade = trade_log.last().unwrap();
                if last_trade.trade_type != "buy" {
                    println!("⚠️ Last trade was not a BUY. Skipping...");
                    return;
                }
            
                let sol_holding = last_trade.amount_token_b; // e.g. 0.03472 SOL
                let paid_usdc = last_trade.amount_token_a;   // e.g. 5.0 USDC
            
                // 2. Get quote for selling that SOL to USDC
                let amount_lamports = (sol_holding * 1_000_000_000.0) as u64;
                let quote_url = format!(
                    "https://quote-api.jup.ag/v6/quote?inputMint={}&outputMint={}&amount={}&slippageBps={}",
                    left_asset,  // SOL
                    right_asset, // USDC
                    amount_lamports,
                    50 // 0.5% slippage tolerance
                );
            
                let client = reqwest::Client::new();
                let quote_resp = client.get(&quote_url).send().await;
                if let Ok(resp) = quote_resp {
                    let quote_json: serde_json::Value = resp.json().await.unwrap();
                    let out_amount_str = quote_json["outAmount"]
                        .as_str()
                        .ok_or("Missing outAmount in quote")
                        .unwrap();
                    let usdc_received = out_amount_str.parse::<f64>().unwrap() / 1_000_000.0;
            
                    println!("🔁 Holding: {:.6} SOL → would return {:.6} USDC", sol_holding, usdc_received);
            
                    // 3. Check if it's profitable
                    let target_return = paid_usdc * (1.0 + sell_percentage / 100.0);
                    println!("🎯 Need at least {:.6} USDC to sell for profit (+{}%)", target_return, sell_percentage);
            
                    if usdc_received >= target_return {
                        println!("✅ SELL opportunity detected!");
            
                        // 4. Execute the swap
                        let mut retries = 200;
                        let mut slippage_bps = 1;
                        let slippage_bps_max = 5;
            
                        while retries > 0 {
                            println!("💰 Attempting to sell {:.6} SOL with {}bps slippage...", sol_holding, slippage_bps);
                            match jupiter_swap(
                                rpc_url,
                                left_asset,  // selling SOL
                                right_asset, // buying USDC
                                sol_holding,
                                slippage_bps,
                                &sol_keypair,
                            ).await {
                                Ok((usdc_received_actual, tx_signature)) => {
                                    let profit = usdc_received_actual - paid_usdc;
                                    println!("💰 SELL completed! Got {:.6} USDC in tx {}", usdc_received_actual, tx_signature);
                                    println!("📈 Profit: +{:.6} USDC (+{:.2}%)", profit, (profit / paid_usdc) * 100.0);
            
                                    log_trade(
                                        &format!("logs/pair_{}_{}_trade_history.json", left_asset, right_asset),
                                        &mut trade_log,
                                        "sell",
                                        sol_holding,
                                        usdc_received_actual,
                                    ).unwrap();
            
                                    write_log(
                                        &format!("logs/pair_{}_{}_value.txt", left_asset, right_asset),
                                        &"0.0".to_string(),
                                    ).unwrap();
            
                                    break;
                                }
                                Err(e) => {
                                    println!("⚠️ Sell attempt failed: {:?}", e);
                                    retries -= 1;
                                    if slippage_bps < slippage_bps_max {
                                        slippage_bps += 1;
                                    }
                                    if retries > 0 {
                                        println!("🔁 Retrying in 2 seconds... ({} retries left)", retries);
                                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                    } else {
                                        println!("❌ Max retries reached. Aborting sell.");
                                        break;
                                    }
                                }
                            }
                        }
                    } else {
                        println!("⏳ Not profitable to sell yet.");
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                } else {
                    println!("❌ Failed to fetch quote for selling.");
                }
            }
            
        }
    }
}
