use std::io::BufRead;

use tracing_subscriber;
use trade_engine::pbcode::oms::{self, PlaceOrderReq, TradePair, oms_service_client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // todo: load config from toml file
    // initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .init();

    // connect [::1]:51230

    // read cmd from stdin:
    //   Direction,AccountID,OrderType,Pair,Price,Quantity,TimeInForce,STPStrategy,PostOnly
    // > Bid,1001,LIMIT,BTC_USDT,30000,0.01,GTK,1,true
    // > Ask,1002,MARKET,BTC_USDT,0,0.1,FOK,0,false
    let stdin = std::io::stdin();

    for line in stdin.lock().lines() {
        tokio::spawn(async move {
            let l = line.unwrap();
            call(l).await.unwrap();
        });
    }

    Ok(())
}

async fn call(line: String) -> Result<(), Box<dyn std::error::Error>> {
    let mut client =
        oms_service_client::OmsServiceClient::connect(format!("http://[::1]:51230")).await?;
    let parts: Vec<&str> = line.trim().split(',').collect();
    if parts.len() < 9 {
        println!("Invalid input, expected 9 fields");
        return Err("Invalid input, expected 9 fields".into());
    }
    let direction = parts[0].to_string();
    let account_id: u64 = parts[1].parse()?;
    let order_type = parts[2].to_string();
    let trade_pair_str = parts.get(3).unwrap().to_string();
    let trade_pair = trade_pair_str.split('_').collect::<Vec<&str>>(); // BASE_QUOTE
    let base = trade_pair[0].to_string();
    let quote = trade_pair[1].to_string();
    let price: f64 = parts[4].parse()?;
    let quantity: f64 = parts[5].parse()?;
    let time_in_force = parts[6].to_string();
    let stp_strategy: i32 = parts[7].parse()?;
    let post_only: bool = parts[8].parse()?;

    let create_ts_us = chrono::Utc::now().timestamp_micros() as u64;
    let response = client
        .place_order(PlaceOrderReq {
            order: Some(oms::Order {
                order_id: "".to_string(), // assigned by OMS
                client_order_id: format!(
                    "cli-{}-{}",
                    account_id,
                    rand::random_range(10000000..99999999u64)
                ),
                direction: match direction.as_str() {
                    "Bid" => oms::Direction::Buy as i32,
                    "Ask" => oms::Direction::Sell as i32,
                    _ => {
                        println!("Invalid direction: {}", direction);
                        return Ok(());
                    }
                },
                account_id,
                order_type: match order_type.as_str() {
                    "LIMIT" => oms::OrderType::Limit as i32,
                    "MARKET" => oms::OrderType::Market as i32,
                    _ => {
                        println!("Invalid order type: {}", order_type);
                        return Ok(());
                    }
                },
                trade_pair: Some(TradePair { base, quote }),
                price: price.to_string(),
                quantity: quantity.to_string(),
                time_in_force: match time_in_force.as_str() {
                    "GTK" => oms::TimeInForce::Gtk as i32,
                    "IOC" => oms::TimeInForce::Ioc as i32,
                    "FOK" => oms::TimeInForce::Fok as i32,
                    _ => {
                        println!("Invalid time in force: {}", time_in_force);
                        return Ok(());
                    }
                },
                stp_strategy,
                post_only,
                seq_id: 0,
                prev_seq_id: 0,
                create_time: create_ts_us.to_string(),
            }),
        })
        .await?;
    println!("Response: {:?}", response.into_inner());
    Ok(())
}
