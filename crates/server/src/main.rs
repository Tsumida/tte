use std::net::SocketAddr;

use rust_decimal_macros::dec;
use tonic::transport::Server;
use tracing::info;
use tte_core::pbcode::oms::match_engine_service_server;

use tte_core::{
    pbcode::oms::{self, oms_service_server},
    types::TradePair,
};
use tte_infra::config::AppConfig;
use tte_infra::kafka::{ConsumerConfig, ProducerConfig};
use tte_me::orderbook;
use tte_me::service::MatchEngineService;
use tte_oms::{oms::OMS, service::TradeSystem};
use tte_rlr::{RaftClient, RaftServer};
use tte_sequencer::raft::RaftSequencerConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::var("SERVER_MODE").as_deref() {
        Ok("oms") => run_oms().await?,

        // --ME_BASE=ETH \
        // --ME_QUOTE=USDT \
        Ok("me") => {
            let base = std::env::var("ME_BASE").unwrap();
            let quote = std::env::var("ME_QUOTE").unwrap();
            run_me(&base, &quote, None).await?
        }
        _ => {
            panic!("unknown SERVER_MODE, please set SERVER_MODE to 'oms' or 'me'");
        }
    }
    Ok(())
}

async fn run_oms() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::dev();
    config
        .with_kafka_producer(
            "match_req_BTCUSDT",
            ProducerConfig {
                trade_pair: TradePair::new("BTC", "USDT"),
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topic: "match_req_BTCUSDT".to_string(),
                acks: -1, // "all"
                message_timeout_ms: 5000,
            },
        )
        .with_kafka_producer(
            "match_req_ETHUSDT",
            ProducerConfig {
                trade_pair: TradePair::new("ETH", "USDT"),
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topic: "match_req_ETHUSDT".to_string(),
                acks: -1, // "all"
                message_timeout_ms: 5000,
            },
        )
        .with_kafka_producer(
            "order_events",
            ProducerConfig {
                // todo: 考虑去掉
                trade_pair: TradePair::new("BTC", "USDT"),
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topic: "order_events".to_string(),
                acks: -1, // "all"
                message_timeout_ms: 5000,
            },
        )
        .with_kafka_producer(
            "ledger_events",
            ProducerConfig {
                // todo: 考虑去掉
                trade_pair: TradePair::new("BTC", "USDT"),
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topic: "ledger_events".to_string(),
                acks: -1, // "all"
                message_timeout_ms: 5000,
            },
        )
        .with_kafka_consumer(
            "match_result_BTCUSDT",
            ConsumerConfig {
                trade_pair: TradePair::new("BTC", "USDT"),
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topics: vec!["match_result_BTCUSDT".to_string()],
                group_id: "oms_match_result".to_string(),
                auto_offset_reset: "earliest".to_string(), // auto
            },
        )
        .with_kafka_consumer(
            "match_result_ETHUSDT",
            ConsumerConfig {
                trade_pair: TradePair::new("ETH", "USDT"),
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topics: vec!["match_result_ETHUSDT".to_string()],
                group_id: "oms_match_result".to_string(),
                auto_offset_reset: "earliest".to_string(), // auto
            },
        );

    let _ = config.init_tracer().await?;
    config.print_args();

    let balances = vec![
        (1000, "BTC", -400.0),
        (1000, "USDT", -4_000_000.0),
        (1000, "ETH", -2_000.0),
        (1001, "BTC", 100.0),
        (1001, "USDT", 1_000_000.0),
        (1001, "ETH", 500.0),
        (1002, "BTC", 100.0),
        (1002, "USDT", 1_000_000.0),
        (1002, "ETH", 500.0),
        (1003, "BTC", 100.0),
        (1003, "USDT", 1_000_000.0),
        (1003, "ETH", 500.0),
        (1004, "BTC", 100.0),
        (1004, "USDT", 1_000_000.0),
        (1004, "ETH", 500.0),
    ];
    let market_data = vec![
        (
            TradePair::new("BTC", "USDT"),
            dec!(80000.0),
            oms::TradePairConfig {
                trade_pair: "BTCUSDT".to_string(),
                min_price_increment: "0.01".to_string(),
                min_quantity_increment: "0.0001".to_string(),
                state: 1,
                volatility_limit: "0.1".to_string(),
            },
        ),
        (
            TradePair::new("ETH", "USDT"),
            dec!(4000.0),
            oms::TradePairConfig {
                trade_pair: "ETHUSDT".to_string(),
                min_price_increment: "0.01".to_string(),
                min_quantity_increment: "0.0001".to_string(),
                state: 1,
                volatility_limit: "0.1".to_string(),
            },
        ),
    ];

    // todo: load OMS from last snapshot
    let mut oms = OMS::new();
    oms.with_init_ledger(balances).with_market_data(market_data);
    let (svc, _bg_tasks) = TradeSystem::run_trade_system(
        oms,
        config
            .kafka_producers()
            .iter()
            .filter(|(_, v)| v.topic().starts_with("match_req_"))
            .map(|(_, v)| (v.trade_pair().clone(), v.clone()))
            .collect(),
        config
            .kafka_consumers()
            .iter()
            .filter(|(_, v)| v.topics()[0].starts_with("match_result_"))
            .map(|(_, v)| (v.trade_pair().clone(), v.clone()))
            .collect(),
        config
            .kafka_producers()
            .iter()
            .find(|(k, _)| *k == "ledger_events")
            .unwrap()
            .1
            .clone(),
        config
            .kafka_producers()
            .iter()
            .find(|(k, _)| *k == "order_events")
            .unwrap()
            .1
            .clone(),
    )
    .await?;
    // rpc handler
    let addr = config.grpc_server_endpoint().parse()?;

    info!("oms listen at {}", addr);
    Server::builder()
        .add_service(oms_service_server::OmsServiceServer::new(svc))
        .serve(addr)
        .await?;

    config.shutdown().await?;
    Ok(())
}

// 每一个交易对一个实例
async fn run_me(
    base: &str,
    quote: &str,
    exit_signal: Option<tokio::sync::oneshot::Receiver<()>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::dev();
    config
        .with_kafka_producer(
            &format!("match_result_{}{}", base, quote),
            ProducerConfig {
                trade_pair: TradePair::new(base, quote),
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topic: format!("match_result_{}{}", base, quote),
                acks: -1, // "all"
                message_timeout_ms: 5000,
            },
        )
        .with_kafka_consumer(
            &format!("match_req_{}{}", base, quote),
            ConsumerConfig {
                trade_pair: TradePair::new(base, quote),
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topics: vec![format!("match_req_{}{}", base, quote)],
                group_id: "oms_match_result".to_string(),
                auto_offset_reset: "earliest".to_string(), // auto
            },
        );
    let raft_config = RaftSequencerConfig::from_env().expect("load raft config"); // todo: from AppConfig

    let _ = config.init_tracer().await?;
    config.print_args();
    let addr = config.grpc_server_endpoint().parse()?;
    let raft_addr: SocketAddr = raft_config
        .nodes()
        .get(&raft_config.node_id())
        .unwrap()
        .rpc_addr
        .parse()?;
    let pair = TradePair::new(base, quote);
    let (me, _bg_tasks) = MatchEngineService::run_match_engine(
        raft_config,
        pair.clone(),
        orderbook::OrderBook::new(pair),
        config.kafka_producers().clone(),
        config.kafka_consumers().clone(),
    )
    .await
    .expect("start match engine service");

    let handles = vec![
        tokio::spawn(async move {
            info!("match-engine listen at {}", addr);
            Server::builder()
                .add_service(match_engine_service_server::MatchEngineServiceServer::new(
                    me,
                ))
                .serve(addr)
                .await
                .unwrap();
            info!("match-engine down");
        }),
        tokio::spawn(async move {
            // info!("raft sequencer listen at {}", raft_addr);
            // Server::builder()
            //     .add_service(RaftServiceServer::new(todo!()))
            //     .serve(raft_addr)
            //     .await
            //     .unwrap();
            // info!("match-engine down");
        }),
    ];

    if let Some(exit_signal) = exit_signal {
        exit_signal.await?;
    } else {
        futures::future::join_all(handles).await;
    }
    Ok(())
}
