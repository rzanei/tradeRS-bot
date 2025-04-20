use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer};

use crate::{
    log_manager::{load_trade_log, log_trade, read_log, write_log},
    trading_math::{decreased_amount_by_percentage, increased_amount_by_percentage},
    utils::{
        check_tx_success, get_pool_assets, get_token_balance, get_token_price_by_pool, get_wallet_balance, pool_swap, sign_tx_broadcast, simulate_swap_math, simulate_swap_via_lcd, sol_get_sol_balance, sol_get_token_balance, wait_for_tx_confirmation
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
    let mint_address = Pubkey::from_str(right_asset).unwrap(); // `right_asset` must be a base58 string
    
    
    // === 1. Parse mnemonic, derive key and address ===
    let sol_keypair = Keypair::from_base58_string(&wallet_pk);
    let wallet_pubkey = sol_keypair.pubkey();
    println!("✅ Connected Wallet Address: {:?}", wallet_pubkey);

    let mut trade_log = load_trade_log(&format!("logs/pair_{left_asset}_{right_asset}_trade_history.json")).unwrap();
    println!("{:?}", trade_log);

    loop {
        let value = read_log(&format!("logs/pair_{left_asset}_{right_asset}_value.txt")).unwrap();

        match value.eq(&0.0) {
            true => {
                println!("Make First Trade");

                let asset_b_balance = sol_get_sol_balance(
                    rpc_url,
                    &wallet_pubkey,
                )
                .await
                .unwrap();
                println!("Account Balance: {:?}", asset_b_balance);

                // if asset_b_balance > buy_amount {
                //     let (account_number, sequence, msg) = pool_swap(
                //         &address,
                //         pool_id,
                //         &asset_b.token.denom.to_string(),
                //         &asset_a.token.denom.to_string(),
                //         buy_amount,
                //         0.001,
                //     )
                //     .await
                //     .unwrap();
                //     let tx_hash =
                //         sign_tx_broadcast(msg, public_key, sequence, account_number, &signing_key)
                //             .await
                //             .unwrap();
                //     let (success, log, maybe_trade_amounts) =
                //         wait_for_tx_confirmation(&tx_hash, 10, 3).await.unwrap();

                //     if success {
                //         let (amount_token_a, amount_token_b) =
                //             maybe_trade_amounts.unwrap_or((0.0, 0.0));
                //         log_trade(
                //             &format!("logs/pool_{pool_id}_trade_history.json"),
                //             &mut trade_log,
                //             "buy",
                //             amount_token_a,
                //             amount_token_b,
                //         )
                //         .unwrap();

                //         write_log(
                //             &format!("logs/pool_{pool_id}_value.txt"),
                //             &amount_token_a.to_string(),
                //         )
                //         .unwrap();
                //     } else {
                //         println!("🚫 Transaction failed or not confirmed: {:?}", log);
                //     }

                //     if success {
                //         println!("🎉 Transaction confirmed!");
                //     } else {
                //         println!("🚫 Transaction failed or not confirmed: {:?}", log);
                //     }
                // } else {
                //     println!(
                //         "Insufficient balance: have {}, need {}",
                //         asset_b_balance, buy_amount
                //     );
                // }
            }

            false => {
            //     println!("📈 Checking SELL conditions...");

            //     // 1. Read what we paid (in OSMO)
            //     let paid_osmo = read_log(&format!("logs/pool_{pool_id}_value.txt")).unwrap();

            //     // 2. Find how much ATOM we have (from last trade)
            //     let last_trade = &trade_log[0];
            //     let atom_holding = last_trade.amount_token_b;

            //     // 3. Calculate target sell price in OSMO
            //     let target_return = paid_osmo * (1.0 + sell_percentage / 100.0);
            //     println!("🎯 Target return: {:.6} OSMO", target_return);

            //     // 4. Simulate the trade
            //     let atom_balance = get_token_balance(&address, &asset_a.token.denom)
            //         .await
            //         .unwrap()
            //         / 1_000_000.0;

            //     if atom_balance >= atom_holding {
            //         let (account_number, sequence, msg) = pool_swap(
            //             &address,
            //             pool_id,
            //             &asset_a.token.denom,
            //             &asset_b.token.denom,
            //             atom_holding,
            //             0.001, // slippage
            //         )
            //         .await
            //         .unwrap();

            //         let expected_osmo = simulate_swap_via_lcd(
            //             msg.clone(),
            //             public_key,
            //             sequence,
            //             account_number,
            //             &signing_key,
            //         )
            //         .await
            //         .unwrap();

            //         println!(
            //             "🔁 Holding: {:.6} ATOM → would return {:.6} OSMO",
            //             atom_holding, expected_osmo
            //         );

            //         // 5. If simulation meets profit target, perform trade
            //         if expected_osmo >= target_return {
            //             println!("✅ SELL opportunity detected!");

            //             let tx_hash = sign_tx_broadcast(
            //                 msg,
            //                 public_key,
            //                 sequence,
            //                 account_number,
            //                 &signing_key,
            //             )
            //             .await
            //             .unwrap();

            //             let (success, log, maybe_trade_amounts) =
            //                 wait_for_tx_confirmation(&tx_hash, 10, 3).await.unwrap();

            //             if success {
            //                 let (osmo_received, _) = maybe_trade_amounts.unwrap_or((0.0, 0.0));

            //                 println!(
            //                     "💰 SOLD {:.6} ATOM for {:.6} OSMO",
            //                     atom_holding, osmo_received
            //                 );

            //                 log_trade(
            //                     &format!("logs/pool_{pool_id}_trade_history.json"),
            //                     &mut trade_log,
            //                     "sell",
            //                     atom_holding,
            //                     osmo_received,
            //                 )
            //                 .unwrap();

            //                 write_log(&format!("logs/pool_{pool_id}_value.txt"), &"0".to_string())
            //                     .unwrap();
            //             } else {
            //                 println!("🚫 SELL failed: {:?}", log);
            //             }
            //         } else {
            //             println!("⏳ Waiting — not profitable to sell yet.");
            //         }
            //     } else {
            //         println!(
            //             "❌ Not enough ATOM in wallet: have {}, need {}",
            //             atom_balance, atom_holding
            //         );
            //     }
            }
        }

        // Optionally sleep before the next check
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}
