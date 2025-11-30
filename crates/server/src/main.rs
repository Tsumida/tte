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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::var("SERVER_MODE").as_deref() {
        Ok("oms") => run_oms().await?,
        Ok("me") => run_me().await?,
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
        .with_kafka_consumer(
            "match_result_BTCUSDT",
            ConsumerConfig {
                trade_pair: TradePair::new("BTC", "USDT"),
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topics: vec!["match_result_BTCUSDT".to_string()],
                group_id: "oms_match_result".to_string(),
                auto_offset_reset: "earliest".to_string(), // auto
            },
        );

    let _ = config.init_tracer().await?;
    config.print_args();

    let balances = vec![
        (1000, "BTC", -400.0),
        (1000, "USDT", -4_000_000.0),
        (1001, "BTC", 100.0),
        (1001, "USDT", 1_000_000.0),
        (1002, "BTC", 100.0),
        (1002, "USDT", 1_000_000.0),
        (1003, "BTC", 100.0),
        (1003, "USDT", 1_000_000.0),
        (1004, "BTC", 100.0),
        (1004, "USDT", 1_000_000.0),
    ];
    let market_data = vec![(
        TradePair::new("BTC", "USDT"),
        dec!(10000.0),
        oms::TradePairConfig {
            trade_pair: "BTCUSDT".to_string(),
            min_price_increment: "0.01".to_string(),
            min_quantity_increment: "0.0001".to_string(),
            state: 1,
            volatility_limit: "0.1".to_string(),
        },
    )];

    // todo: load OMS from last snapshot
    let mut oms = OMS::new();
    oms.with_init_ledger(balances).with_market_data(market_data);
    let (svc, _bg_tasks) = TradeSystem::run_trade_system(
        oms,
        config
            .kafka_producers()
            .iter()
            .map(|(_, v)| (v.trade_pair().clone(), v.clone()))
            .collect(),
        config
            .kafka_consumers()
            .iter()
            .map(|(_, v)| (v.trade_pair().clone(), v.clone()))
            .collect(),
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

async fn run_me() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::dev();
    config
        .with_kafka_producer(
            "match_result_BTCUSDT",
            ProducerConfig {
                trade_pair: TradePair::new("BTC", "USDT"),
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topic: "match_result_BTCUSDT".to_string(),
                acks: -1, // "all"
                message_timeout_ms: 5000,
            },
        )
        .with_kafka_consumer(
            "match_req_BTCUSDT",
            ConsumerConfig {
                trade_pair: TradePair::new("BTC", "USDT"),
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topics: vec!["match_req_BTCUSDT".to_string()],
                group_id: "oms_match_result".to_string(),
                auto_offset_reset: "earliest".to_string(), // auto
            },
        );

    let _ = config.init_tracer().await?;
    config.print_args();
    let addr = config.grpc_server_endpoint().parse()?;

    let pair = TradePair::new("BTC", "USDT");
    let (me, _bg_tasks) = MatchEngineService::run_match_engine(
        pair.clone(),
        orderbook::OrderBook::new(pair),
        config.kafka_producers().clone(),
        config.kafka_consumers().clone(),
    )
    .await
    .expect("start match engine service");

    info!("match-engine listen at {}", addr);
    Server::builder()
        .add_service(match_engine_service_server::MatchEngineServiceServer::new(
            me,
        ))
        .serve(addr)
        .await?;

    info!("match-engine down");
    config.shutdown().await?;
    Ok(())
}
