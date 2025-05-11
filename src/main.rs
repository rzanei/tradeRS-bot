use std::{
    io::{self, Write},
    thread::sleep,
    time::Duration,
};

use jupiter_strategy_start::jup_bot_start;
use osmosis_strategy_start::osmo_bot_start;

mod jupiter_strategy_start;
mod log_manager;
mod osmosis_strategy_start;
mod market_risk_analyzer;
mod utils;

#[tokio::main]
async fn main() {
    loop {
        println!("Please Select an option");
        println!("1. Start Osmosis Bot [ATOM/OSMO]");
        println!("2. Start Jupiter Bot [SOL/USDC]");
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
                        osmo_bot_start(
                            pool_id,
                            amount_token_a,
                            sell_percentage,
                            buy_percentage,
                            recover_percentage,
                        )
                        .await;
                    }
                    Err(err) => {
                        eprintln!("Error fetching pool data: {}", err);
                    }
                }
            }
            "2" => {
                let left_asset = "So11111111111111111111111111111111111111112";
                let right_asset = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

                println!("Starting Jupiter Bot [{left_asset}/{right_asset}] ...");

                // Trading parameters
                let amount_token_a: f64 = 100.0; // This is the initial buy of the trade bot 
                let sell_percentage: f64 = 0.3; // Sell percentage (The Profit Percentage)
                let dca_recover_percentage: f64 = 1.5; // Percentage to trigger the DCA Buy if market goes down (Risk Management Strategy)
                let dca_recover_percentage_to_buy: f64 = 10.0; // Percentage of the total capital to buy as recovery (i.e dca_recover_percentage of amount_token_a )

                println!("Running strategy with parameters:");
                println!("- Assets: [{left_asset}/{right_asset}]");
                println!("- amount_token_a: {}", amount_token_a);
                println!("- sell_percentage: {}%", sell_percentage);
                println!("- dca_recover_percentage: {}%", dca_recover_percentage);
                println!("- dca_recover_percentage_to_buy: {}%", dca_recover_percentage_to_buy);
                jup_bot_start(
                    left_asset,
                    right_asset,
                    sell_percentage,
                    dca_recover_percentage,
                    dca_recover_percentage_to_buy,
                )
                .await;
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
