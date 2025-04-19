use std::{io::{self, Write}, thread::sleep, time::Duration};

use strategy_1_start::osmo_bot_start;

mod utils;
mod strategy_1_start;
mod log_manager;
mod trading_math;

#[tokio::main]
async fn main() {
    loop {
        println!("Please Select an option");
        println!("1. Start Osmosis Bot [USDC/ATOM] on Port: [9222]");
        println!("0. Exit");

        io::stdout().flush().unwrap();
        let mut option = String::new();
        io::stdin().read_line(&mut option).unwrap();

        match option.trim() {
            "1" => {
                let pool_id = "1";

                println!("Starting Osmosis Bot [Pool: {:?}] ...", pool_id);

                // Trading parameters
                let amount_token_a: f64 = 90.0; // Amount per trade (USDC)
                let sell_percentage: f64 = 0.5;
                let buy_percentage: f64 = 2.5;
                let recover_percentage: f64 = 65.0;

                // Fetch pool data
                match utils::get_token_price_by_pool(pool_id).await {
                    Ok(_) => {
                        println!("Running strategy with parameters:");
                        println!("- Pool ID: {}", pool_id);
                        println!("- amount_token_a: {}", amount_token_a);
                        println!("- sell_percentage: {}%", sell_percentage);
                        println!("- buy_percentage: {}%", buy_percentage);
                        println!("- recover_percentage: {}%", recover_percentage);
                        osmo_bot_start(pool_id,  amount_token_a,  sell_percentage, buy_percentage,  recover_percentage).await;
                    }
                    Err(err) => {
                        eprintln!("Error fetching pool data: {}", err);
                    }
                }
            }
            "0" => {
                println!("Exiting bot...");
                break;
            }
            _ => {
                println!("Invalid option. Try again.");
            }
        }

        // Optional pause between loops
        sleep(Duration::from_secs(1));
    }
}
