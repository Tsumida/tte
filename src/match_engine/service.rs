//#![allow(dead_code)]
#![deny(clippy::unwrap_used)]

use std::{collections::HashMap, sync::Arc};

use tokio::{sync::RwLock, task::JoinHandle};

use crate::{
    common::types::TradePair,
    infra::kafka::{ConsumerConfig, ProducerConfig},
    match_engine::orderbook::{OrderBook, OrderBookErr},
};

pub struct MatchEngineService {
    ob_view: Arc<RwLock<OrderBook>>,
}

impl MatchEngineService {
    pub async fn init() -> Self {
        // let orderbook = Arc::new(RwLock::new(OrderBook::new().await));
        // MatchEngineService { orderbook }
        todo!()
    }

    // todo:
    pub async fn run_match_engine(
        orderbook: OrderBook,
        producer_cfgs: HashMap<String, ProducerConfig>,
        consumer_cfgs: HashMap<String, ConsumerConfig>,
    ) -> Result<(Self, Vec<JoinHandle<()>>), OrderBookErr> {
        let ob = Arc::new(RwLock::new(orderbook));
        // init Sequencer, ApplyThread, MatchResultProducer, ConsumerTask
        todo!()
    }
}

struct ApplyThread {
    orderbook: Arc<RwLock<OrderBook>>,
}

struct MatchResultProducer {
    trade_pair: TradePair,
    cfg: ProducerConfig,
    producer: rdkafka::producer::FutureProducer,
}
