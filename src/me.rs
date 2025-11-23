use rust_decimal_macros::dec;
use tonic::transport::Server;
use tracing::info;
use trade_engine::infra::config::AppConfig;
use trade_engine::infra::kafka::{ConsumerConfig, ProducerConfig};
use trade_engine::match_engine::{orderbook, service};
use trade_engine::{
    common::types::TradePair,
    pbcode::oms::{self, oms_service_server},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    // todo: load OMS from last snapshot
    let (svc, _bg_tasks) = service::MatchEngineService::run_match_engine(
        orderbook::OrderBook::new(),
        config.kafka_producers().clone(),
        config.kafka_consumers().clone(),
    )
    .await
    .expect("start match engine service");
    // rpc handler
    // let addr = config.grpc_server_endpoint().parse()?;

    // info!("oms listen at {}", addr);
    // Server::builder()
    //     .add_service(oms_service_server::OmsServiceServer::new(svc))
    //     .serve(addr)
    //     .await?;

    config.shutdown().await?;
    Ok(())
}
