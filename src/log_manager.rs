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

pub async fn telegram_command_listener(trading_flag: Arc<Mutex<bool>>) {
    if let Err(e) = dotenvy::from_path(".env") {
        if cfg!(debug_assertions) {
            eprintln!("⚠️  .env file not found: {e}");
        }
    }

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
    if let Err(e) = dotenvy::from_path(".env") {
        if cfg!(debug_assertions) {
            eprintln!("⚠️  .env file not found: {e}");
        }
    }
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

// End of Telegram API
