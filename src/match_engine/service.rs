//#![allow(dead_code)]
#![deny(clippy::unwrap_used)]

use std::{collections::HashMap, sync::Arc};

use rdkafka::producer;
use tokio::{sync::RwLock, task::JoinHandle};

use crate::{
    common::{err_code, types::TradePair},
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

    pub async fn run_match_engine(
        trade_pair: TradePair,
        orderbook: OrderBook,
        producer_cfgs: HashMap<String, ProducerConfig>,
        consumer_cfgs: HashMap<String, ConsumerConfig>,
    ) -> Result<(Self, Vec<JoinHandle<()>>), OrderBookErr> {
        let ob = Arc::new(RwLock::new(orderbook));
        let apply_thread = ApplyThread::new(ob.clone());

        let req_topic = format!("match_req_{}{}", trade_pair.base, trade_pair.quote);
        let result_topic = format!("match_result_{}{}", trade_pair.base, trade_pair.quote);
        let req_consumer_cfg = consumer_cfgs
            .get(&req_topic)
            .expect("missing consumer config");
        let result_producer_cfg = producer_cfgs
            .get(&result_topic)
            .expect("missing producer config");

        let match_result_producer =
            MatchResultProducerBuilder::with_producer_cfg(result_producer_cfg)
                .build()
                .expect("create producer");

        let match_req_consumer = MatchConsumerBuilder::with_consumer_cfg(req_consumer_cfg)
            .build()
            .expect("create consumer");

        let handlers = vec![
            tokio::spawn(async move {
                apply_thread.run().await;
            }),
            tokio::spawn(async move {
                match_result_producer.run().await;
            }),
            tokio::spawn(async move {
                match_req_consumer.run().await;
            }),
        ];

        // init Sequencer, ApplyThread, MatchResultProducer, ConsumerTask
        Ok((MatchEngineService { ob_view: ob }, handlers))
    }
}

struct MatchConsumerBuilder<'a> {
    cfg: &'a ConsumerConfig,
}

impl<'a> MatchConsumerBuilder<'a> {
    pub fn with_consumer_cfg(cfg: &'a ConsumerConfig) -> Self {
        MatchConsumerBuilder { cfg }
    }

    pub fn build(self) -> Result<MatchReqConsumer, Box<dyn std::error::Error>> {
        let consumer = self.cfg.subscribe()?;
        Ok(MatchReqConsumer {
            trade_pair: self.cfg.trade_pair.clone(),
            consumer,
        })
    }
}

struct MatchReqConsumer {
    trade_pair: TradePair,
    consumer: rdkafka::consumer::StreamConsumer,
}

impl MatchReqConsumer {
    pub async fn run(self) {
        todo!()
    }
}

struct ApplyThread {
    orderbook: Arc<RwLock<OrderBook>>,
}

impl ApplyThread {
    pub fn new(orderbook: Arc<RwLock<OrderBook>>) -> Self {
        ApplyThread { orderbook }
    }

    pub async fn run(self) {
        todo!()
    }
}

struct MatchResultProducer {
    trade_pair: TradePair,
    producer: rdkafka::producer::FutureProducer,
}

impl MatchResultProducer {
    pub async fn run(self) {
        todo!()
    }
}

struct MatchResultProducerBuilder<'a> {
    cfg: &'a ProducerConfig,
}

impl<'a> MatchResultProducerBuilder<'a> {
    pub fn with_producer_cfg(cfg: &'a ProducerConfig) -> Self {
        MatchResultProducerBuilder { cfg }
    }

    pub fn build(self) -> Result<MatchResultProducer, Box<dyn std::error::Error>> {
        let producer = self.cfg.create_producer()?;
        Ok(MatchResultProducer {
            trade_pair: self.cfg.trade_pair.clone(),
            producer,
        })
    }
}
