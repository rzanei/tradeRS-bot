// use crate::{
//     log_manager::{load_trade_log, log_trade, read_log, write_log},
//     utils::{
//         get_pool_assets, get_token_balance, pool_swap, sign_tx_broadcast, simulate_swap_via_lcd,
//         wait_for_tx_confirmation,
//     },
// };
// use std::env;

// pub async fn osmo_bot_start(
//     pool_id: &str,
//     buy_amount: f64,
//     sell_percentage: f64,
//     _buy_percentage: f64,
//     _recover_percentage: f64,
// ) {
//     dotenvy::from_path(".env").expect("Failed to load .env");
//     let wallet_mnemo = env::var("WALLET_MNEMO").expect("WALLET_MNEMO not set in .env");

//     use bip32::{DerivationPath, XPrv};
//     use bip39::Mnemonic;
//     use cosmrs::crypto::secp256k1::SigningKey;
//     use std::str::FromStr;

//     // === 1. Parse mnemonic, derive key and address ===

//     let mnemonic = Mnemonic::parse(wallet_mnemo).unwrap();
//     let seed = mnemonic.to_seed("");
//     let derivation_path = DerivationPath::from_str("m/44'/118'/0'/0/0").unwrap();
//     let child_xprv = XPrv::derive_from_path(seed, &derivation_path).unwrap();
//     let _xprv = XPrv::new(&seed).unwrap();
//     let signing_key = SigningKey::from_slice(&child_xprv.to_bytes()).unwrap();
//     let public_key = signing_key.public_key();
//     let account_id = public_key.account_id("osmo").unwrap();
//     let address = account_id.to_string();

//     println!("✅ Connected Wallet Address: {}", address);

//     let mut trade_log = load_trade_log(&format!("logs/pool_{pool_id}_trade_history.json")).unwrap();
//     println!("{:?}", trade_log);

//     loop {
//         let value = read_log(&format!("logs/pool_{pool_id}_value.txt")).unwrap();
//         let (asset_a, asset_b) = get_pool_assets("1").await.unwrap(); // [asset_a: ATOM/asset_b: OSMO]
//         print!("Asset_a: {:?}, Asset_b: {:?}", asset_a, asset_b);

//         match value.eq(&0.0) {
//             true => {
//                 println!("Make First Trade");

//                 let asset_b_balance = get_token_balance(
//                     "osmo1xxhe4ffac4uuulrr6er08kf8e2j8q0pt47ledr",
//                     &asset_b.token.denom,
//                 )
//                 .await
//                 .unwrap()
//                     / 1_000_000.0;

//                 println!("Account Balance: {:?}", asset_b_balance);

//                 if asset_b_balance > buy_amount {
//                     let (account_number, sequence, msg) = pool_swap(
//                         &address,
//                         pool_id,
//                         &asset_b.token.denom.to_string(),
//                         &asset_a.token.denom.to_string(),
//                         buy_amount,
//                         0.001,
//                     )
//                     .await
//                     .unwrap();
//                     let tx_hash =
//                         sign_tx_broadcast(msg, public_key, sequence, account_number, &signing_key)
//                             .await
//                             .unwrap();
//                     let (success, log, maybe_trade_amounts) =
//                         wait_for_tx_confirmation(&tx_hash, 10, 3).await.unwrap();

//                     if success {
//                         let (amount_token_a, amount_token_b) =
//                             maybe_trade_amounts.unwrap_or((0.0, 0.0));
//                         log_trade(
//                             &format!("logs/pool_{pool_id}_trade_history.json"),
//                             &mut trade_log,
//                             "buy",
//                             amount_token_a,
//                             amount_token_b,
//                             Some(current_dca_level),
//                         )
//                         .unwrap();

//                         write_log(
//                             &format!("logs/pool_{pool_id}_value.txt"),
//                             &amount_token_a.to_string(),
//                         )
//                         .unwrap();
//                     } else {
//                         println!("🚫 Transaction failed or not confirmed: {:?}", log);
//                     }

//                     if success {
//                         println!("🎉 Transaction confirmed!");
//                     } else {
//                         println!("🚫 Transaction failed or not confirmed: {:?}", log);
//                     }
//                 } else {
//                     println!(
//                         "Insufficient balance: have {}, need {}",
//                         asset_b_balance, buy_amount
//                     );
//                 }
//             }

//             false => {
//                 println!("📈 Checking SELL conditions...");

//                 // 1. Read what we paid (in OSMO)
//                 let paid_osmo = read_log(&format!("logs/pool_{pool_id}_value.txt")).unwrap();

//                 // 2. Find how much ATOM we have (from last trade)
//                 let last_trade = &trade_log[0];
//                 let atom_holding = last_trade.amount_token_b;

//                 // 3. Calculate target sell price in OSMO
//                 let target_return = paid_osmo * (1.0 + sell_percentage / 100.0);
//                 println!("🎯 Target return: {:.6} OSMO", target_return);

//                 // 4. Simulate the trade
//                 let atom_balance = get_token_balance(&address, &asset_a.token.denom)
//                     .await
//                     .unwrap()
//                     / 1_000_000.0;

//                 if atom_balance >= atom_holding {
//                     let (account_number, sequence, msg) = pool_swap(
//                         &address,
//                         pool_id,
//                         &asset_a.token.denom,
//                         &asset_b.token.denom,
//                         atom_holding,
//                         0.001, // slippage
//                     )
//                     .await
//                     .unwrap();

//                     let expected_osmo = simulate_swap_via_lcd(
//                         msg.clone(),
//                         public_key,
//                         sequence,
//                         account_number,
//                         &signing_key,
//                     )
//                     .await
//                     .unwrap();

//                     println!(
//                         "🔁 Holding: {:.6} ATOM → would return {:.6} OSMO",
//                         atom_holding, expected_osmo
//                     );

//                     // 5. If simulation meets profit target, perform trade
//                     if expected_osmo >= target_return {
//                         println!("✅ SELL opportunity detected!");

//                         let tx_hash = sign_tx_broadcast(
//                             msg,
//                             public_key,
//                             sequence,
//                             account_number,
//                             &signing_key,
//                         )
//                         .await
//                         .unwrap();

//                         let (success, log, maybe_trade_amounts) =
//                             wait_for_tx_confirmation(&tx_hash, 10, 3).await.unwrap();

//                         if success {
//                             let (osmo_received, _) = maybe_trade_amounts.unwrap_or((0.0, 0.0));

//                             println!(
//                                 "💰 SOLD {:.6} ATOM for {:.6} OSMO",
//                                 atom_holding, osmo_received
//                             );

//                             log_trade(
//                                 &format!("logs/pool_{pool_id}_trade_history.json"),
//                                 &mut trade_log,
//                                 "sell",
//                                 atom_holding,
//                                 osmo_received,
//                                 Some(current_dca_level),
//                             )
//                             .unwrap();

//                             write_log(&format!("logs/pool_{pool_id}_value.txt"), &"0".to_string())
//                                 .unwrap();
//                         } else {
//                             println!("🚫 SELL failed: {:?}", log);
//                         }
//                     } else {
//                         println!("⏳ Waiting — not profitable to sell yet.");
//                     }
//                 } else {
//                     println!(
//                         "❌ Not enough ATOM in wallet: have {}, need {}",
//                         atom_balance, atom_holding
//                     );
//                 }
//             }
//         }

//         // Optionally sleep before the next check
//         tokio::time::sleep(std::time::Duration::from_secs(10)).await;
//     }
// }
