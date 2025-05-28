use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::Path;
use std::sync::Arc;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
};
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Trade {
    pub trade_type: String,
    pub amount_token_a: f64,
    pub amount_token_b: f64,
    pub time: String,
    pub dca_level: Option<u32>,
}

pub fn write_log(file_path: &str, log: &str) -> io::Result<()> {
    let mut file_path = File::create(file_path)?; // Open the file in write mode (OVERWRITE!)
    file_path.write_all(log.as_bytes())?; // Write the log as bytes
    Ok(())
}

pub fn read_log(file_path: &str) -> io::Result<f64> {
    if let Ok(mut file) = File::open(file_path) {
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        let number: f64 = content.trim().parse().unwrap_or(0.0); // Default to 0.0 if parsing fails
        Ok(number)
    } else {
        File::create(file_path).unwrap();
        Ok(0 as f64)
    }
}

pub fn append_log(file_path: &str, log: &str) -> io::Result<()> {
    let mut file_path = OpenOptions::new()
        .append(true)
        .create(true)
        .open(file_path)?; // Open the file in append mode
    file_path.write_all(log.as_bytes())?; // Write the log as bytes
    Ok(())
}

pub fn log_trade(
    file_path: &str,
    trade_log: &mut Vec<Trade>,
    trade_type: &str,
    amount_token_a: f64,
    amount_token_b: f64,
    dca_level: Option<u32>,
) -> io::Result<()> {
    let trade = Trade {
        trade_type: trade_type.to_string(),
        amount_token_a,
        amount_token_b,
        time: Utc::now().to_rfc3339(),
        dca_level,
    };
    trade_log.push(trade.clone()); // Append the trade to the log (list)
    let trade_json = serde_json::to_string(&trade)?; // Serialize the trade to JSON
    append_log(file_path, &format!("{}\n", trade_json))?; // Append trade to the log file
    Ok(())
}

pub fn load_trade_log(file_path: &str) -> io::Result<Vec<Trade>> {
    if Path::new(file_path).exists() {
        let mut file = File::open(file_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let trades: Vec<Trade> = contents
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok()) // Deserialize each line into a Trade object
            .collect();

        Ok(trades) // ✅ Return the full trade history
    } else {
        println!("load_trade_log: File {} does not exist.", file_path);
        File::create(file_path).unwrap();
        Ok(Vec::new())
    }
}

// Start of Telegram API

#[derive(Debug, Deserialize)]
struct Update {
    update_id: i64,
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    text: Option<String>,
    chat: Chat,
}

#[derive(Debug, Deserialize)]
struct Chat {
    id: i64,
}

pub async fn telegram_command_listener(left_asset: &str, right_asset: &str, sell_percentage: f64, trading_flag: Arc<Mutex<bool>>) {
    // if let Err(e) = dotenvy::from_path(".env") {
    //     if cfg!(debug_assertions) {
    //         eprintln!("⚠️  .env file not found: {e}");
    //     }
    // }

    let client = Client::new();
    let mut last_update_id: i64 = 0;
    let telegram_http_api =
        env::var("TELEGRAM_HTTP_API").expect("TELEGRAM_HTTP_API not set in .env");
    let telegram_chat_id = env::var("TELEGRAM_CHAT_ID").expect("TELEGRAM_CHAT_ID not set in .env");

    loop {
        let url = format!(
            "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout=10",
            telegram_http_api,
            last_update_id + 1
        );

        match client.get(&url).send().await {
            Ok(response) => {
                if let Ok(json) = response.json::<serde_json::Value>().await {
                    if let Some(results) = json["result"].as_array() {
                        for update in results {
                            let update: Update = serde_json::from_value(update.clone()).unwrap();
                            if let Some(msg) = &update.message {
                                if msg.chat.id.to_string() == telegram_chat_id {
                                    if let Some(text) = &msg.text {
                                        match text.as_str() {
                                            "/status" => {
                                                let status = if *trading_flag.lock().await {
                                                    "🟢 Bot is Online"
                                                } else {
                                                    "🔴 Bot is Offline"
                                                };
                                                send_telegram_message(status).await.ok();
                                            }
                                            "/start_trading" => {
                                                *trading_flag.lock().await = true;
                                                send_telegram_message("✅ Trading Started")
                                                    .await
                                                    .ok();
                                            }
                                            "/stop_trading" => {
                                                let mut flag = trading_flag.lock().await;
                                                if *flag {
                                                    *flag = false;
                                                    send_telegram_message("🛑 Safe Stop Triggered")
                                                        .await
                                                        .ok();
                                                } else {
                                                    send_telegram_message(
                                                        "⚠️ Trading already stopped.",
                                                    )
                                                    .await
                                                    .ok();
                                                }
                                            }
                                            "/market_status" => {
                                                match generate_market_status(left_asset, right_asset, sell_percentage).await {
                                                    Ok(summary) => {
                                                        send_telegram_message(&summary).await.ok();
                                                    }
                                                    Err(e) => {
                                                        send_telegram_message(&format!(
                                                            "❌ Failed to get market status: {e}"
                                                        ))
                                                        .await
                                                        .ok();
                                                    }
                                                }
                                            }

                                            _ => {}
                                        }
                                    }
                                }
                            }
                            last_update_id = update.update_id;
                        }
                    }
                }
            }
            Err(e) => eprintln!("Error checking updates: {:?}", e),
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

pub async fn send_telegram_message(message: &str) -> Result<(), reqwest::Error> {
    // if let Err(e) = dotenvy::from_path(".env") {
    //     if cfg!(debug_assertions) {
    //         eprintln!("⚠️  .env file not found: {e}");
    //     }
    // }
    let telegram_http_api =
        env::var("TELEGRAM_HTTP_API").expect("TELEGRAM_HTTP_API not set in .env");
    let telegram_chat_id = env::var("TELEGRAM_CHAT_ID").expect("TELEGRAM_CHAT_ID not set in .env");

    let url = format!(
        "https://api.telegram.org/bot{}/sendMessage",
        telegram_http_api
    );

    let client = reqwest::Client::new();
    let res = client
        .post(&url)
        .form(&[
            ("chat_id", telegram_chat_id),
            ("text", message.to_string()),
            ("parse_mode", "Markdown".to_string()),
        ])
        .send()
        .await?;

    if res.status().is_success() {
        println!("✅ Message sent to Telegram!");
    } else {
        eprintln!(
            "❌ Failed to send: {}",
            res.text().await.unwrap_or_default()
        );
    }

    Ok(())
}

pub async fn generate_market_status(left_asset: &str, right_asset: &str, sell_percentage: f64) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use crate::log_manager::{load_trade_log, read_log};

    let sol_holding = read_log(&format!(
        "logs/solana/pair_{}_{}_value.txt",
        left_asset, right_asset
    ))?;

    let trade_log = load_trade_log(&format!(
        "logs/solana/pair_{}_{}_trade_history.json",
        left_asset, right_asset
    ))?;

    let open_trades: Vec<&_> =
        if let Some(last_sell_idx) = trade_log.iter().rposition(|t| t.trade_type == "sell") {
            trade_log.iter().skip(last_sell_idx + 1).collect()
        } else {
            trade_log.iter().collect()
        };

    let paid_usdc: f64 = open_trades.iter().map(|trade| trade.amount_token_a).sum();

    let amount_lamports = (sol_holding * 1_000_000_000.0) as u64;
    let quote_url = format!(
        "https://quote-api.jup.ag/v6/quote?inputMint={}&outputMint={}&amount={}&slippageBps={}",
        left_asset, right_asset, amount_lamports, 50
    );

    let client = reqwest::Client::new();
    let quote_resp = client.get(&quote_url).send().await?;
    let quote_json: serde_json::Value = quote_resp.json().await?;
    let out_amount_str = quote_json["outAmount"]
        .as_str()
        .ok_or("Missing outAmount")?;
    let usdc_received = out_amount_str.parse::<f64>()? / 1_000_000.0;

    let target_return = paid_usdc * (1.0 + sell_percentage / 100.0);
    let price_change = if paid_usdc > 0.0 {
        100.0 * (usdc_received / paid_usdc - 1.0)
    } else {
        0.0
    };

    Ok(format!(
        "🔁 Holding: {:.6} SOL →\n\
         🔁 Would return {:.6} USDC for selling {:.6} SOL\n\
         🎯 Need at least {:.6} USDC to sell for profit (+{:.1}%)\n\
         📉 Price is at {:+.2}%",
        sol_holding, usdc_received, sol_holding, target_return, sell_percentage, price_change
    ))
}

// End of Telegram API
