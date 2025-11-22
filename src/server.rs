use rust_decimal_macros::dec;
use tonic::transport::Server;
use tracing::info;
use trade_engine::infra::config::AppConfig;
use trade_engine::infra::kafka::{ConsumerConfig, ProducerConfig};
use trade_engine::{
    common::types::TradePair,
    oms::{oms::OMS, service},
    pbcode::oms::{self, oms_service_server},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = AppConfig::dev();
    config
        .with_kafka_producer(
            "match_req_BTCUSDT",
            ProducerConfig {
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topic: "match_requests".to_string(),
                acks: -1, // "all"
                message_timeout_ms: 5000,
            },
        )
        .with_kafka_consumer(
            "match_result_BTCUSDT",
            ConsumerConfig {
                bootstrap_servers: "kafka-dev:9092".to_string(),
                topics: vec!["match_results".to_string()],
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

    let (svc, _bg_tasks) = service::TradeSystem::run_trade_system(
        oms,
        config.kafka_producers().clone(),
        config.kafka_consumers().clone(),
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
