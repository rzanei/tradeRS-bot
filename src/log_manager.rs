use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
};

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
