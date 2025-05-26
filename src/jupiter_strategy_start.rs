use chrono::{DateTime, Utc};
use solana_sdk::{signature::Keypair, signer::Signer};

use crate::{
    log_manager::{load_trade_log, log_trade, read_log, send_telegram_message, write_log},
    market_risk_analyzer::{
        PriceTouchAnalyzer, fetch_and_log_binance_history, fetch_current_binance_price_from_log,
    },
    utils::{get_usdc_balance, jupiter_swap, sol_get_sol_balance},
};
use std::env;

pub async fn jup_bot_start(
    left_asset: &str,
    right_asset: &str,
    sell_percentage: f64,
    dca_recover_percentage: f64,
    r_factor: f64, // Each DCA level increases USDC amount by r_factor (e.g., 0.5 for 50%)
) {

    send_telegram_message(&format!("🟢 TradeRS-bot Online")).await.unwrap();

    dotenvy::from_path(".env").expect("Failed to load .env");
    let wallet_pk = env::var("SOL_WALLET_PK").expect("SOL_WALLET_PK not set in .env");
    let rpc_url = "https://api.mainnet-beta.solana.com";

    // === 1. Parse mnemonic, derive key and address ===
    let sol_keypair = Keypair::from_base58_string(&wallet_pk);
    let wallet_pubkey = sol_keypair.pubkey();
    println!("✅ Connected Wallet Address: {:?}", wallet_pubkey);
    let dca_level_path = format!(
        "logs/solana/pair_{}_{}_dca_level.txt",
        left_asset, right_asset
    );
    let mut current_dca_level: u32 = match std::fs::read_to_string(&dca_level_path) {
        Ok(content) => content.trim().parse().unwrap_or(0),
        Err(_) => 0,
    };

    let mut trade_log = load_trade_log(&format!(
        "logs/solana/pair_{left_asset}_{right_asset}_trade_history.json"
    ))
    .unwrap();
    println!("{:?}", trade_log);

    send_telegram_message(&format!("⌛ Loading Configuration... ")).await.unwrap();

    loop {
        let value = read_log(&format!(
            "logs/solana/pair_{left_asset}_{right_asset}_value.txt"
        ))
        .unwrap();
        let cooldown_secs = 3600; // 1 hour
        let now = Utc::now();

        // Trade data section
        let mut trade_retries = 200;
        let mut trade_slippage_bps = 1;
        let trade_slippage_bps_max = 5;
        let mut smart_adjusted_amount = 0.0;
        let mut tmp_multip = 0.0;

        match value.eq(&0.0) {
            true => {
                // === 1. Cooldown Check ===
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

                // === 2. Market Risk Check ===
                println!("🕒 Checking for Market Condition...");
                let binance_price_log =
                    format!("logs/solana/binance_{left_asset}_{right_asset}__prices.csv");

                if let Err(e) =
                    fetch_and_log_binance_history(&binance_price_log, "SOLUSDT", "4").await
                {
                    println!("⚠️ Failed to fetch Binance history: {}", e);
                    continue;
                }

                if let Ok(analyzer) = PriceTouchAnalyzer::from_file(&binance_price_log, 0.25) {
                    let current_price =
                        fetch_current_binance_price_from_log(&binance_price_log).unwrap();
                    let (risk_label, touches, multiplier) =
                        analyzer.assess_price(current_price, sell_percentage);
                    tmp_multip = multiplier; // f64 is Copy, no need to clone
                    let log_line = format!(
                        "[Risk Check] Price: {:.2} | Touches: {} | Risk: {} | Multiplier: {:.2}",
                        current_price, touches, risk_label, multiplier
                    );
                    println!("{}", log_line);

                    // Calculate the adjusted amount to use based on current USDC balance
                    let adjusted_amount = get_usdc_balance(
                        &wallet_pubkey.to_string(),
                        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                    )
                    .await
                        * multiplier;

                    match risk_label.as_str() {
                        "🔴 HIGH-RISK" => {
                            println!("❌ Skipping trade due to HIGH RISK.");
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue;
                        }
                        "⚠️ WEAK ZONE" => {
                            println!(
                                "⚠️ Weak zone detected. Reducing trade size to {:.2}.",
                                adjusted_amount
                            );
                        }
                        _ => {
                            println!(
                                "✅ Risk acceptable. Using adjusted size: {:.2}",
                                adjusted_amount
                            );
                        }
                    }

                    // Ensure the adjusted amount meets a minimum threshold
                    if adjusted_amount < 10.0 {
                        println!(
                            "⚠️ Adjusted amount {:.2} too small to execute. Skipping.",
                            adjusted_amount
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                        continue;
                    } else {
                        smart_adjusted_amount = adjusted_amount;
                    }
                }

                // === 3. Proceed with First Trade (Market Buy) ===
                println!("🚀 Market & Risk Passed. Preparing to Buy...");

                let asset_b_balance = sol_get_sol_balance(rpc_url, &wallet_pubkey).await.unwrap();
                println!("Account Balance: {:?} SOL", asset_b_balance);

                while trade_retries > 0 {
                    println!(
                        "💵 Attempting to buy {:.6} worth of {} with slippage {}bps",
                        smart_adjusted_amount, left_asset, trade_slippage_bps
                    );

                    match jupiter_swap(
                        rpc_url,
                        right_asset, // USDC – what you have
                        left_asset,  // SOL – what you want to buy
                        smart_adjusted_amount,
                        trade_slippage_bps,
                        &sol_keypair,
                    )
                    .await
                    {
                        Ok((received_amount, tx_signature)) => {
                            println!(
                                "🎉 Buy successful! Received {:.6} {} in tx {}",
                                received_amount, left_asset, tx_signature
                            );

                            // Log the buy trade
                            log_trade(
                                &format!(
                                    "logs/solana/pair_{}_{}_trade_history.json",
                                    left_asset, right_asset
                                ),
                                &mut trade_log,
                                "buy",
                                smart_adjusted_amount, // USDC spent
                                received_amount,       // SOL received
                                Some(current_dca_level),
                            )
                            .unwrap();

                            // Telegram notificator
                            send_telegram_message(&format!(
                                "🎉 *Buy successful!*\nReceived `{:.6}` *{}* in tx:\n`{}`",
                                received_amount, left_asset, tx_signature
                            ))
                            .await
                            .unwrap();

                            // Write the newly received SOL to the value file
                            let current_val = value;
                            let new_val = current_val + received_amount;
                            write_log(
                                &format!(
                                    "logs/solana/pair_{}_{}_value.txt",
                                    left_asset, right_asset
                                ),
                                &new_val.to_string(),
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
                                println!(
                                    "🔁 Retrying in 2 seconds... ({} trade_retries left)",
                                    trade_retries
                                );
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

                // ---- NEW: Compute the open position only using trades after the most recent SELL ----
                let open_trades: Vec<&_> = if let Some(last_sell_idx) =
                    trade_log.iter().rposition(|t| t.trade_type == "sell")
                {
                    trade_log.iter().skip(last_sell_idx + 1).collect()
                } else {
                    trade_log.iter().collect()
                };

                let sol_holding = read_log(&format!(
                    "logs/solana/pair_{}_{}_value.txt",
                    left_asset, right_asset
                ))
                .unwrap();
                let paid_usdc: f64 = open_trades.iter().map(|trade| trade.amount_token_a).sum();
                let average_entry_price = paid_usdc / sol_holding;

                println!("🔁 Holding: {:.6} SOL → ", sol_holding,);

                // === 2. Get quote for selling that SOL to USDC ===
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
                        "🔁 Would return {:.6} USDC for selling {:.6} SOL",
                        usdc_received, sol_holding
                    );

                    // === 3. Check if it's profitable ===
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

                                    // Reset DCA state upon full exit
                                    std::fs::write(&dca_level_path, "0").unwrap();
                                    current_dca_level = 0;
                                    println!(
                                        "📈 Profit: +{:.6} USDC (+{:.2}%)",
                                        profit,
                                        (profit / paid_usdc) * 100.0
                                    );

                                    log_trade(
                                        &format!(
                                            "logs/solana/pair_{}_{}_trade_history.json",
                                            left_asset, right_asset
                                        ),
                                        &mut trade_log,
                                        "sell",
                                        sol_holding,
                                        usdc_received_actual,
                                        Some(current_dca_level),
                                    )
                                    .unwrap();

                                    write_log(
                                        &format!(
                                            "logs/solana/pair_{}_{}_value.txt",
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

                        if price_change <= -dca_recover_percentage {
                            println!("🛒 DCA Triggered! Buying the dip...");
                            current_dca_level += 1;

                            // Save updated DCA level
                            std::fs::write(&dca_level_path, current_dca_level.to_string()).unwrap();

                            let tmp_multip = if tmp_multip == 0.0 { 1.0 } else { tmp_multip };
                            let dca_amount = (get_usdc_balance(
                                &wallet_pubkey.to_string(),
                                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                            )
                            .await
                                * tmp_multip)
                                * r_factor;
                            if dca_amount < 5.0 {
                                println!("⚠️ DCA amount too small ({:.2}). Skipping.", dca_amount);
                                continue;
                            }
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

                                        // Log the DCA buy trade
                                        log_trade(
                                            &format!(
                                                "logs/solana/pair_{}_{}_trade_history.json",
                                                left_asset, right_asset
                                            ),
                                            &mut trade_log,
                                            "buy",
                                            dca_amount,
                                            received_amount,
                                            Some(current_dca_level),
                                        )
                                        .unwrap();

                                        // Update the value.txt by adding the new SOL received
                                        let current_val = value;
                                        let new_val = current_val + received_amount;
                                        write_log(
                                            &format!(
                                                "logs/solana/pair_{}_{}_value.txt",
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
                                            println!(
                                                "❌ Max trade_retries reached for DCA. Aborting."
                                            );
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
