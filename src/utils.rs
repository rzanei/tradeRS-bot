use reqwest::Error;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct PoolAsset {
    token: Token,
    weight: String,
}

#[derive(Debug, Deserialize)]
struct Token {
    denom: String,
    amount: String,
}

#[derive(Debug, Deserialize)]
struct Pool {
    pool_assets: Vec<PoolAsset>,
}

#[derive(Debug, Deserialize)]
struct PoolResponse {
    pool: Pool,
}

pub async fn get_token_price_by_pool(pool_id: &str) -> Result<(), Error> {
    let url = format!("https://osmosis-api.polkachu.com/osmosis/gamm/v1beta1/pools/{}", pool_id);

    let res = reqwest::get(&url).await?.json::<PoolResponse>().await?;

    println!("Pool assets: {:?}", res.pool.pool_assets);

    if res.pool.pool_assets.len() == 2 {
        let token_a = &res.pool.pool_assets[0];
        let token_b = &res.pool.pool_assets[1];

        let amount_a: f64 = token_a.token.amount.parse().unwrap();
        let amount_b: f64 = token_b.token.amount.parse().unwrap();

        let price = amount_b / amount_a;
        println!(
            "1 {} ≈ {} {}",
            token_a.token.denom, price, token_b.token.denom
        );
    } else {
        println!("Pool doesn't have exactly 2 assets — handle accordingly.");
    }

    Ok(())
}
