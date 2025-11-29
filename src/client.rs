use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;

use clap::Parser;
use tonic::transport::Channel;
use tracing_subscriber;
use trade_engine::pbcode::oms::{
    self, PlaceOrderReq, TradePair, match_engine_service_client, oms_service_client,
};

/// 命令行参数结构体
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 指令流文件路径，例如 "place.case"
    #[clap(value_parser)]
    file_path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 解析命令行参数
    let args = Args::parse();

    // todo: load config from toml file
    // initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .init();

    // 初始化 gRPC 客户端连接
    let oms_client = oms_service_client::OmsServiceClient::connect(format!("http://[::1]:8080"))
        .await
        .expect("init oms client failed");
    let me_client = match_engine_service_client::MatchEngineServiceClient::connect(format!(
        "http://[::1]:8081"
    ))
    .await
    .expect("init me client failed");

    // 确定输入源：文件或标准输入
    let reader: Box<dyn BufRead> = match args.file_path {
        Some(path) => {
            tracing::info!("Reading commands from file: {:?}", path);
            let file = File::open(path)?;
            Box::new(BufReader::new(file))
        }
        None => {
            tracing::info!("Reading commands from stdin (Ctrl+D to exit)");
            Box::new(io::stdin().lock())
        }
    };

    // 遍历输入流中的每一行
    for line_result in reader.lines() {
        let l = line_result?;
        if l.trim().is_empty() {
            continue;
        }

        // 克隆客户端连接的 Channel (用于并发)
        // 注意：tonic/grpc-rs 的客户端是可克隆的，克隆它们是推荐的并发使用方式。
        let mut oms_client_clone = oms_client.clone();
        let mut me_client_clone = me_client.clone();
        let command = l.trim().to_string(); // 拷贝指令字符串

        // 创建异步任务并存储句柄
        let _ = if command.starts_with("OMS:Bid") || command.starts_with("OMS:Ask") {
            match send_oms_trade_cmd(command[4..].to_string(), &mut oms_client_clone).await {
                Ok(_) => tracing::info!("OMS command processed successfully: {}", command),
                Err(e) => tracing::error!("Error processing OMS command: {} -> {}", command, e),
            }
        } else if command.starts_with("OMS:Snapshot") {
            match send_oms_admin_cmd(command[4..].to_string(), &mut oms_client_clone).await {
                Ok(_) => tracing::info!("OMS admin command processed successfully: {}", command),
                Err(e) => {
                    tracing::error!("Error processing OMS admin command: {} -> {}", command, e)
                }
            }
        } else if command.starts_with("ME:") {
            match send_me_admin_cmd(command[3..].to_string(), &mut me_client_clone).await {
                Ok(_) => tracing::info!("ME command processed successfully: {}", command),
                Err(e) => tracing::error!("Error processing ME command: {} -> {}", command, e),
            }
        } else {
            println!("Unknown command prefix, please use 'OMS:' or 'ME:'");
        };
    }

    // 等待所有异步任务完成
    // futures::future::join_all(tasks).await;

    Ok(())
}

// ----------------------------------------------------------------------
// call 和 call_me 函数保持不变，但为了完整性，在此列出并略作修改以使用 tracing::error
// ----------------------------------------------------------------------

async fn send_oms_trade_cmd(
    line: String,
    client: &mut oms_service_client::OmsServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = line.trim().split(',').collect();
    if parts.len() < 9 {
        tracing::error!("Invalid input, expected 9 fields for OMS: {}", line);
        return Err("Invalid input, expected 9 fields".into());
    }

    // 解析指令字段
    let direction = parts[0];
    let account_id: u64 = parts[1]
        .parse()
        .map_err(|e| format!("AccountID parse error: {}", e))?;
    let order_type = parts[2];
    let trade_pair_str = parts.get(3).ok_or("Missing TradePair")?;
    let trade_pair: Vec<&str> = trade_pair_str.split('_').collect(); // BASE_QUOTE
    if trade_pair.len() != 2 {
        tracing::error!("Invalid TradePair format: {}", trade_pair_str);
        return Err("Invalid TradePair format".into());
    }
    let base = trade_pair[0].to_string();
    let quote = trade_pair[1].to_string();
    let price: f64 = parts[4]
        .parse()
        .map_err(|e| format!("Price parse error: {}", e))?;
    let quantity: f64 = parts[5]
        .parse()
        .map_err(|e| format!("Quantity parse error: {}", e))?;
    let time_in_force = parts[6];
    let stp_strategy: i32 = parts[7]
        .parse()
        .map_err(|e| format!("STPStrategy parse error: {}", e))?;
    let post_only: bool = parts[8]
        .parse()
        .map_err(|e| format!("PostOnly parse error: {}", e))?;

    // 映射枚举值
    let dir = match direction {
        "Bid" => oms::Direction::Buy,
        "Ask" => oms::Direction::Sell,
        _ => {
            tracing::error!("Invalid direction: {}", direction);
            return Err(format!("Invalid direction: {}", direction).into());
        }
    } as i32;

    let ord_type = match order_type {
        "LIMIT" => oms::OrderType::Limit,
        "MARKET" => oms::OrderType::Market,
        _ => {
            tracing::error!("Invalid order type: {}", order_type);
            return Err(format!("Invalid order type: {}", order_type).into());
        }
    } as i32;

    let tif = match time_in_force {
        "GTK" => oms::TimeInForce::Gtk,
        "IOC" => oms::TimeInForce::Ioc,
        "FOK" => oms::TimeInForce::Fok,
        _ => {
            tracing::error!("Invalid time in force: {}", time_in_force);
            return Err(format!("Invalid time in force: {}", time_in_force).into());
        }
    } as i32;

    let create_ts_us = chrono::Utc::now().timestamp_micros() as u64;
    let client_order_id_suffix = create_ts_us;

    let response = client
        .place_order(PlaceOrderReq {
            order: Some(oms::Order {
                order_id: "".to_string(), // assigned by OMS
                client_order_id: format!("cli-{}-{}", account_id, client_order_id_suffix),
                direction: dir,
                account_id,
                order_type: ord_type,
                trade_pair: Some(TradePair { base, quote }),
                price: format!("{:.2}", price),       // 统一格式化价格
                quantity: format!("{:.4}", quantity), // 统一格式化数量
                time_in_force: tif,
                stp_strategy,
                post_only,
                trade_id: 0,
                prev_trade_id: 0,
                create_time: create_ts_us.to_string(),
            }),
        })
        .await?;

    tracing::info!("OMS Response: {:?}", response.into_inner());
    Ok(())
}

async fn send_oms_admin_cmd(
    line: String,
    client: &mut oms_service_client::OmsServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = line.trim().split(',').collect();

    if parts[0] == "Snapshot" {
        let response = client
            .take_snapshot(oms::TakeSnapshotReq {})
            .await
            .expect("take snapshot failed");
        tracing::info!("OrderBook Snapshot: {:?}", response.into_inner());
    } else {
        tracing::error!("Unknown OMS command: {}", parts[0]);
    }

    Ok(())
}

async fn send_me_admin_cmd(
    line: String,
    client: &mut match_engine_service_client::MatchEngineServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = line.trim().split(',').collect();

    if parts[0] == "Snapshot" {
        let response = client
            .take_snapshot(oms::TakeSnapshotReq {})
            .await
            .expect("take snapshot failed");
        tracing::info!("OrderBook Snapshot: {:?}", response.into_inner());
    } else {
        tracing::error!("Unknown ME command: {}", parts[0]);
    }

    Ok(())
}
