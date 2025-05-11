use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};

// === CONFIG ===
const MAX_HISTORY_LINES: usize = 1000;

/// Struct to analyze how many times a price bucket has been touched.
pub struct PriceTouchAnalyzer {
    pub price_history: Vec<f64>,
    pub bucket_size: f64,
}

/// Fetches recent price history from Binance and overwrites the local log file
pub async fn fetch_and_log_binance_history(
    path: &str,
    symbol: &str,
    timeframe: &str,
) -> Result<(), String> {
    let url = format!(
        "https://api.binance.com/api/v3/klines?symbol={}&interval={}m&limit={}", // change here m/h
        symbol, timeframe, MAX_HISTORY_LINES
    );

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to fetch Binance history: {}", e))?;

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;

    let arr = json
        .as_array()
        .ok_or("Unexpected JSON format from Binance")?;

    let mut lines = vec![];
    for candle in arr {
        if let Some(close_str) = candle.get(4).and_then(|v| v.as_str()) {
            if let Ok(close) = close_str.parse::<f64>() {
                lines.push(format!("{:.6}", close));
            }
        }
    }

    if lines.is_empty() {
        return Err("No valid price data fetched.".into());
    }

    let content = lines.join("\n") + "\n";
    fs::write(path, content).map_err(|e| format!("Failed to write price log: {}", e))
}

/// Appends the current price to a log file
fn _append_price_to_log(path: &str, price: f64) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(|e| format!("Failed to open log file: {}", e))?;
    writeln!(file, "{:.6}", price).map_err(|e| format!("Failed to write to log: {}", e))?;
    Ok(())
}

/// Keeps only the last `n` lines in the file
fn _trim_price_log(path: &str, max_lines: usize) -> Result<(), String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read log: {}", e))?;

    let lines: Vec<_> = content.lines().rev().take(max_lines).collect();
    let trimmed: String = lines
        .into_iter()
        .rev()
        .map(|l| format!("{}\n", l))
        .collect();

    fs::write(path, trimmed).map_err(|e| format!("Failed to write trimmed log: {}", e))
}

/// Reads the most recent price from log file
pub fn fetch_current_binance_price_from_log(path: &str) -> Result<f64, String> {
    let file = File::open(path).map_err(|e| format!("Could not open price log: {}", e))?;
    let reader = BufReader::new(file);

    let last_line = reader
        .lines()
        .filter_map(|l| l.ok())
        .filter(|l| !l.trim().is_empty())
        .last()
        .ok_or("Price log is empty")?;

    last_line
        .trim()
        .parse::<f64>()
        .map_err(|e| format!("Failed to parse last price: {}", e))
}

impl PriceTouchAnalyzer {
    /// Load price history from a file (one price per line)
    pub fn from_file(path: &str, bucket_size: f64) -> Result<Self, String> {
        let contents = fs::read_to_string(path).map_err(|e| format!("Failed to read: {}", e))?;
        let mut prices = vec![];

        for line in contents.lines() {
            if let Ok(p) = line.trim().parse::<f64>() {
                prices.push(p);
            }
        }

        if prices.is_empty() {
            return Err("Price history is empty.".into());
        }

        Ok(Self {
            price_history: prices,
            bucket_size,
        })
    }

    /// Build a map of price buckets and how many times they were touched
    pub fn bucket_counts(&self) -> HashMap<u64, usize> {
        let mut map = HashMap::new();
        for &price in &self.price_history {
            let bucket = self.bucket_price(price);
            *map.entry(bucket).or_insert(0) += 1;
        }
        map
    }

    /// Assess the risk level and suggest position multiplier
    pub fn assess_price(&self, current_price: f64, sell_percentage: f64) -> (String, usize, f64) {
        let target_price = current_price * (1.0 + sell_percentage / 100.0);
        let bucketed_price = self.bucket_price(target_price);
        let counts = self.bucket_counts();
        let touch_count = *counts.get(&bucketed_price).unwrap_or(&0);

        // The more the target price has been hit before, the safer the entry now
        let (risk_label, position_multiplier) = match touch_count {
            0..=2 => ("🔴 HIGH-RISK", 0.25), // Very rare — target might not be realistic
            3..=6 => ("🟡 MODERATE", 0.5),  // Possible, but still risky
            7..=15 => ("🟢 SAFE", 0.75),   // Often touched — reliable zone
            _ => ("✅ VERY SAFE", 1.0),      // Heavily tested — highly probable to hit
        };

        (risk_label.to_string(), touch_count, position_multiplier)
    }

    fn bucket_price(&self, price: f64) -> u64 {
        ((price / self.bucket_size).round() * self.bucket_size * 1000.0) as u64
    }
}
