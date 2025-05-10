use chrono::{DateTime, Utc};
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::{
    log_manager::{load_trade_log, log_trade, read_log, write_log},
    trading_math::{decreased_amount_by_percentage, increased_amount_by_percentage},
    utils::{jupiter_swap, sol_get_sol_balance},
};
use std::env;

pub async fn jup_bot_start(
    left_asset: &str,
    right_asset: &str,
    amount_token_a: f64,
    sell_percentage: f64,
    dca_recover_percentage: f64,
    dca_recover_percentage_to_buy: f64,
) {
    dotenvy::from_path(".env").expect("Failed to load .env");
    let wallet_pk = env::var("SOL_WALLET_PK").expect("SOL_WALLET_PK not set in .env");
    let rpc_url = "https://api.mainnet-beta.solana.com";

    // === 1. Parse mnemonic, derive key and address ===
    let sol_keypair = Keypair::from_base58_string(&wallet_pk);
    let wallet_pubkey = sol_keypair.pubkey();
    println!("✅ Connected Wallet Address: {:?}", wallet_pubkey);

    let mut trade_log = load_trade_log(&format!(
        "logs/pair_{left_asset}_{right_asset}_trade_history.json"
    ))
    .unwrap();
    println!("{:?}", trade_log);

    loop {
        let value = read_log(&format!("logs/pair_{left_asset}_{right_asset}_value.txt")).unwrap();
        let cooldown_secs = 3600; // 1 hour
        let now = Utc::now();

        // Trade data section
        let mut trade_retries = 200;
        let mut trade_slippage_bps = 1;
        let trade_slippage_bps_max = 5;
        

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

                let asset_b_balance = sol_get_sol_balance(rpc_url, &wallet_pubkey).await.unwrap();
                println!("Account Balance: {:?} SOL", asset_b_balance);

                while trade_retries > 0 {
                    println!(
                        "💵 Attempting to buy {:.6} worth of {} with slippage {}bps",
                        amount_token_a, left_asset, trade_slippage_bps
                    );

                    match jupiter_swap(
                        rpc_url,
                        right_asset, // USDC - what you have
                        left_asset,  // SOL - what you want to buy
                        amount_token_a,
                        trade_slippage_bps,
                        &sol_keypair,
                    )
                    .await
                    {
                        Ok((received_amount, tx_signature)) => {
                            println!(
                                "🎉 Buy successful! Received {:.6} {} in tx {}",
                                received_amount, right_asset, tx_signature
                            );

                            log_trade(
                                &format!(
                                    "logs/pair_{}_{}_trade_history.json",
                                    left_asset, right_asset
                                ),
                                &mut trade_log,
                                "buy",
                                amount_token_a,  // Correct: USDC spent
                                received_amount, // Correct: SOL received
                            )
                            .unwrap();

                            write_log(
                                &format!("logs/pair_{}_{}_value.txt", left_asset, right_asset),
                                &received_amount.to_string(),
                            )
                            .unwrap();

                            break;
                        }
                        Err(e) => {
                            println!("⚠️ Swap attempt failed: {:?}", e);

                            trade_retries -= 1;
                            if trade_slippage_bps < trade_slippage_bps_max {
                                trade_slippage_bps += 1;
                            }

                            if trade_retries > 0 {
                                println!("🔁 Retrying in 2 seconds... ({} trade_retries left)", trade_retries);
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            } else {
                                println!("❌ Max trade_retries reached. Aborting swap.");
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
                let paid_usdc = last_trade.amount_token_a; // e.g. 5.0 USDC

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

                    println!(
                        "🔁 Holding: {:.6} SOL → would return {:.6} USDC",
                        sol_holding, usdc_received
                    );

                    // 3. Check if it's profitable
                    let target_return = paid_usdc * (1.0 + sell_percentage / 100.0);
                    println!(
                        "🎯 Need at least {:.6} USDC to sell for profit (+{}%)",
                        target_return, sell_percentage
                    );

                    if usdc_received >= target_return {
                        println!("✅ SELL opportunity detected!");

                        while trade_retries > 0 {
                            println!(
                                "💰 Attempting to sell {:.6} SOL with {}bps slippage...",
                                sol_holding, trade_slippage_bps
                            );
                            match jupiter_swap(
                                rpc_url,
                                left_asset,  // selling SOL
                                right_asset, // buying USDC
                                sol_holding,
                                trade_slippage_bps,
                                &sol_keypair,
                            )
                            .await
                            {
                                Ok((usdc_received_actual, tx_signature)) => {
                                    let profit = usdc_received_actual - paid_usdc;
                                    println!(
                                        "💰 SELL completed! Got {:.6} USDC in tx {}",
                                        usdc_received_actual, tx_signature
                                    );
                                    println!(
                                        "📈 Profit: +{:.6} USDC (+{:.2}%)",
                                        profit,
                                        (profit / paid_usdc) * 100.0
                                    );

                                    log_trade(
                                        &format!(
                                            "logs/pair_{}_{}_trade_history.json",
                                            left_asset, right_asset
                                        ),
                                        &mut trade_log,
                                        "sell",
                                        sol_holding,
                                        usdc_received_actual,
                                    )
                                    .unwrap();

                                    write_log(
                                        &format!(
                                            "logs/pair_{}_{}_value.txt",
                                            left_asset, right_asset
                                        ),
                                        &"0.0".to_string(),
                                    )
                                    .unwrap();

                                    break;
                                }
                                Err(e) => {
                                    println!("⚠️ Sell attempt failed: {:?}", e);
                                    trade_retries -= 1;
                                    if trade_slippage_bps < trade_slippage_bps_max {
                                        trade_slippage_bps += 1;
                                    }
                                    if trade_retries > 0 {
                                        println!(
                                            "🔁 Retrying in 2 seconds... ({} trade_retries left)",
                                            trade_retries
                                        );
                                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                    } else {
                                        println!("❌ Max trade_retries reached. Aborting sell.");
                                        break;
                                    }
                                }
                            }
                        }
                    } else {
                        let price_change = 100.0 * (usdc_received / paid_usdc - 1.0);
                        println!("📉 Price is at {:+.2}%", price_change);

                        if price_change >= dca_recover_percentage {
                            println!("🛒 DCA Triggered! Buying the dip...");

                            let dca_amount =
                                amount_token_a * (dca_recover_percentage_to_buy / 100.0);
                            println!("🔁 DCA Buy: Investing {:.2} USDC", dca_amount);

                            while trade_retries > 0 {
                                println!(
                                    "💵 DCA: Attempting to buy {:.6} worth of {} with slippage {}bps",
                                    dca_amount, left_asset, trade_slippage_bps
                                );
                                match jupiter_swap(
                                    rpc_url,
                                    right_asset,
                                    left_asset,
                                    dca_amount,
                                    trade_slippage_bps,
                                    &sol_keypair,
                                )
                                .await
                                {
                                    Ok((received_amount, tx_signature)) => {
                                        println!(
                                            "🎯 DCA buy successful! Got {:.6} {} in tx {}",
                                            received_amount, left_asset, tx_signature
                                        );

                                        // Log the DCA buy
                                        log_trade(
                                            &format!(
                                                "logs/pair_{}_{}_trade_history.json",
                                                left_asset, right_asset
                                            ),
                                            &mut trade_log,
                                            "buy",
                                            dca_amount,
                                            received_amount,
                                        )
                                        .unwrap();

                                        // Update the value.txt (add the received amount)
                                        let current_val = value; // value is already read from file as f64
                                        let new_val = current_val + received_amount;

                                        write_log(
                                            &format!(
                                                "logs/pair_{}_{}_value.txt",
                                                left_asset, right_asset
                                            ),
                                            &new_val.to_string(),
                                        )
                                        .unwrap();

                                        break;
                                    }
                                    Err(e) => {
                                        println!("⚠️ DCA Swap failed: {:?}", e);
                                        trade_retries -= 1;
                                        if trade_slippage_bps < trade_slippage_bps_max {
                                            trade_slippage_bps += 1;
                                        }
                                        if trade_retries > 0 {
                                            println!(
                                                "🔁 Retrying DCA in 2 seconds... ({} trade_retries left)",
                                                trade_retries
                                            );
                                            tokio::time::sleep(std::time::Duration::from_secs(2))
                                                .await;
                                        } else {
                                            println!("❌ Max trade_retries reached for DCA. Aborting.");
                                            break;
                                        }
                                    }
                                }
                            }
                        } else {
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                } else {
                    println!("❌ Failed to fetch quote for selling.");
                }
            }
        }
    }
}
