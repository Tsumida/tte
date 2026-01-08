use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;

use clap::Parser;
use tonic::transport::Channel;
use tracing_subscriber;
use tte_core::pbcode::oms::match_engine_service_client::MatchEngineServiceClient;
use tte_core::pbcode::oms::{self, match_engine_service_client, oms_service_client};

mod parser;
mod smart;

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

    tracing_subscriber::fmt()
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .init();

    let oms_client = oms_service_client::OmsServiceClient::connect(format!("http://[::1]:8080"))
        .await
        .expect("init oms client failed");
    let mut me_clients = init_me_client(
        "BTC_USDT",
        [
            (1, "http://[::1]:8081"),
            (2, "http://[::1]:8082"),
            (3, "http://[::1]:8083"),
        ],
    )
    .await?;
    // map.insert("ETH_USDT".to_string(), me_client_eth_usdt);

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

    for line_result in reader.lines() {
        let l = line_result?;
        if l.trim().is_empty() {
            continue;
        }

        let mut oms_client_clone = oms_client.clone();
        let command = l.trim().to_string(); // 拷贝指令字符串

        // 创建异步任务并存储句柄

        match true {
            // ignore comments
            _ if command.starts_with("//") => {}
            _ if command.starts_with("OMS:Bid") || command.starts_with("OMS:Ask") => {
                match place_order(command[4..].to_string(), &mut oms_client_clone).await {
                    Ok(_) => tracing::info!("OMS command processed successfully: {}", command),
                    Err(e) => tracing::error!("Error processing OMS command: {} -> {}", command, e),
                }
            }
            _ if command.starts_with("OMS:Cancel") => {
                match cancel_order(command[10..].to_string(), &mut oms_client_clone).await {
                    Ok(_) => {
                        tracing::info!("OMS cancel command processed successfully: {}", command)
                    }
                    Err(e) => {
                        tracing::error!("Error processing OMS cancel command: {} -> {}", command, e)
                    }
                }
            }
            _ if command.starts_with("OMS:Snapshot") => {
                match send_oms_admin_cmd(command[4..].to_string(), &mut oms_client_clone).await {
                    Ok(_) => {
                        tracing::info!("OMS admin command processed successfully: {}", command)
                    }
                    Err(e) => {
                        tracing::error!("Error processing OMS admin command: {} -> {}", command, e)
                    }
                }
            }
            _ if command.starts_with("ME:") => {
                // ME:Snapshot,BTC_USDT
                let args: Vec<&str> = command[3..].split(',').collect();
                let market = args[1];

                let clients = me_clients.get_mut(market).unwrap();
                match send_me_admin_cmd(command[3..].to_string(), clients).await {
                    Ok(_) => {
                        tracing::info!("ME command processed successfully: {}", command)
                    }
                    Err(e) => {
                        tracing::error!("Error processing ME command: {} -> {}", command, e)
                    }
                }
            }
            _ => {
                tracing::error!("Unknown command prefix in line: {}", command);
            }
        }
    }
    Ok(())
}

async fn init_me_client(
    trade_pair: &str,
    addrs: [(u64, &str); 3],
) -> Result<HashMap<String, HashMap<String, MatchEngineServiceClient<Channel>>>, anyhow::Error> {
    let mut clients = HashMap::new(); // 3节点
    for (node_id, addr) in addrs {
        let me_client =
            match_engine_service_client::MatchEngineServiceClient::connect(addr.to_string())
                .await
                .expect("init me client failed");
        clients.insert(node_id.to_string(), me_client);
    }

    Ok(HashMap::from([(trade_pair.to_string(), clients)]))
}

async fn place_order(
    line: String,
    client: &mut oms_service_client::OmsServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let req = parser::FileParser::new().parse_fill(line.trim().split(',').collect())?;
    tracing::info!("Req{:?}", req);
    let response = client.place_order(req).await.expect("place order failed");
    tracing::info!("PlaceOrderRsp{:?}", response.into_inner());

    Ok(())
}

async fn cancel_order(
    line: String,
    client: &mut oms_service_client::OmsServiceClient<Channel>,
) -> Result<(), Box<dyn std::error::Error>> {
    let req = parser::FileParser::new().parse_cancel(line.trim().split(',').collect())?;
    let response = client.cancel_order(req).await.expect("cancel order failed");
    tracing::info!("CancelOrderRsp{:?}", response.into_inner());

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
    clients: &mut HashMap<String, match_engine_service_client::MatchEngineServiceClient<Channel>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = line.trim().split(',').collect();
    if parts[0] == "Snapshot" {
        for (node_id, client) in clients.iter_mut() {
            let response = client
                .take_snapshot(oms::TakeSnapshotReq {})
                .await
                .expect("take snapshot failed");
            tracing::info!(
                "OrderBook Snapshot at node {}: {:?}",
                node_id,
                response.into_inner()
            );
        }
    } else {
        tracing::error!("Unknown ME command: {}", parts[0]);
    }

    Ok(())
}
