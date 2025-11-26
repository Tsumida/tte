//#![allow(dead_code)]
#![deny(clippy::unwrap_used)]

use std::collections::HashMap;

use rdkafka::Message;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{info, instrument};

use crate::{
    common::types::{BatchMatchReqTransfer, BatchMatchResultTransfer, MatchResult, TradePair},
    infra::kafka::{ConsumerConfig, ProducerConfig, print_kafka_msg_meta},
    match_engine::orderbook::{OrderBook, OrderBookErr},
    pbcode::oms,
    sequencer::api::{DefaultSequencer, SequenceSetter},
};

#[derive(Debug, Clone)]
pub enum MatchCmd {
    MatchReq(oms::BatchMatchRequest),
    MatchAdminCmd(oms::MatchAdminCmd),
}

struct CmdWrapper<T: Send + Sync + 'static> {
    inner: T,
    // crtical: 此seq_id只用于ME内部故障恢复和幂等
    seq_id: u64,
    prev_seq_id: u64,
}

impl<T> CmdWrapper<T>
where
    T: Send + Sync + 'static,
{
    pub fn new(inner: T) -> Self {
        CmdWrapper {
            inner,
            seq_id: 0,
            prev_seq_id: 0,
        }
    }
}

impl<T> SequenceSetter for CmdWrapper<T>
where
    T: Send + Sync + 'static,
{
    fn set_seq_id(&mut self, seq_id: u64, prev_seq_id: u64) {
        self.seq_id = seq_id;
        self.prev_seq_id = prev_seq_id;
    }
}

type MatchCmdSender = tokio::sync::mpsc::Sender<CmdWrapper<MatchCmd>>;
type MatchCmdReceiver = tokio::sync::mpsc::Receiver<CmdWrapper<MatchCmd>>;
type SequencerSender = tokio::sync::mpsc::Sender<CmdWrapper<MatchCmd>>;
type SequencerReceiver = tokio::sync::mpsc::Receiver<CmdWrapper<MatchCmd>>;
type MatchResultSender = tokio::sync::mpsc::Sender<CmdWrapper<oms::BatchMatchResult>>;
type MatchResultReceiver = tokio::sync::mpsc::Receiver<CmdWrapper<oms::BatchMatchResult>>;

pub struct MatchEngineService {
    #[allow(dead_code)]
    submit_sender: MatchCmdSender, // todo: admin cmd
}

impl MatchEngineService {
    pub async fn run_match_engine(
        trade_pair: TradePair,
        orderbook: OrderBook,
        producer_cfgs: HashMap<String, ProducerConfig>,
        consumer_cfgs: HashMap<String, ConsumerConfig>,
    ) -> Result<(Self, Vec<JoinHandle<()>>), OrderBookErr> {
        let batch_size = 32;
        // 发送到sequencer
        // sequencer持久化结果发送
        let req_topic = format!("match_req_{}{}", trade_pair.base, trade_pair.quote);
        let result_topic = format!("match_result_{}{}", trade_pair.base, trade_pair.quote);
        let req_consumer_cfg = consumer_cfgs
            .get(&req_topic)
            .expect("missing match-engine consumer config");
        let result_producer_cfg = producer_cfgs
            .get(&result_topic)
            .expect("missing match-engine producer config");

        let (sequencer, sequencer_sender, sequencer_receiver) =
            DefaultSequencer::<CmdWrapper<MatchCmd>>::new(0, batch_size);

        let (match_result_sender, match_result_receiver) =
            mpsc::channel::<CmdWrapper<oms::BatchMatchResult>>(batch_size);

        let apply_thread = ApplyThread::new(orderbook, sequencer_receiver, match_result_sender);

        let match_result_producer = MatchResultProducerBuilder::new()
            .with_producer_cfg(result_producer_cfg)
            .with_match_result_receiver(match_result_receiver)
            .build()
            .expect("create producer");

        let match_req_consumer = MatchConsumerBuilder::new()
            .with_consumer_cfg(req_consumer_cfg)
            .with_sequencer_sender(&sequencer_sender)
            .build()
            .expect("create consumer");

        let handlers = vec![
            tokio::spawn(async move {
                match_req_consumer.run().await;
            }),
            tokio::spawn(async move {
                sequencer.run().await;
            }),
            tokio::spawn(async move {
                apply_thread.run().await;
            }),
            tokio::spawn(async move {
                match_result_producer.run().await;
            }),
        ];

        // init Sequencer, ApplyThread, MatchResultProducer, ConsumerTask
        Ok((
            MatchEngineService {
                submit_sender: sequencer_sender,
            },
            handlers,
        ))
    }
}

struct MatchConsumerBuilder<'a> {
    cfg: Option<&'a ConsumerConfig>,
    sender: Option<&'a SequencerSender>,
}

impl<'a> MatchConsumerBuilder<'a> {
    pub fn new() -> Self {
        MatchConsumerBuilder {
            cfg: None,
            sender: None,
        }
    }

    pub fn with_consumer_cfg(mut self, cfg: &'a ConsumerConfig) -> Self {
        self.cfg = Some(cfg);
        self
    }

    pub fn with_sequencer_sender(mut self, sender: &'a SequencerSender) -> Self {
        self.sender = Some(sender);
        self
    }

    pub fn build(self) -> Result<MatchReqConsumer, Box<dyn std::error::Error>> {
        let cfg = self.cfg.expect("consumer config");
        let consumer = cfg.subscribe()?;
        Ok(MatchReqConsumer {
            trade_pair: cfg.trade_pair.clone(),
            consumer,
            sequencer_sender: self.sender.expect("sequencer sender").clone(),
        })
    }
}

struct MatchReqConsumer {
    trade_pair: TradePair,
    consumer: rdkafka::consumer::StreamConsumer,
    sequencer_sender: SequencerSender,
}

impl MatchReqConsumer {
    pub async fn run(self) {
        info!("MatchReqConsumer_{} started", &self.trade_pair.pair());
        loop {
            let r = self.consumer.recv().await;
            match r {
                Err(e) => {
                    info!("Error receiving message: {:?}", e);
                    break;
                }
                Ok(m) => {
                    print_kafka_msg_meta(&m);
                    if let Some(payload) = m.payload() {
                        self.process_kafka_msg(payload).await;
                    }
                }
            }
        }
        drop(self.sequencer_sender);
        info!("MatchReqConsumer_{} stopped", &self.trade_pair.pair());
    }

    #[instrument(level = "info", skip_all)]
    async fn process_kafka_msg(&self, payload: &[u8]) {
        let batch_match_req = BatchMatchReqTransfer::deserialize(payload).expect("deserialize");
        let cmd = MatchCmd::MatchReq(batch_match_req);
        let cmd_wrapper = CmdWrapper::new(cmd);
        self.sequencer_sender
            .send(cmd_wrapper)
            .await
            .expect("send to sequencer");
    }
}

struct ApplyThread {
    orderbook: OrderBook,
    submit_receiver: SequencerReceiver,
    match_result_sender: MatchResultSender,
}

impl ApplyThread {
    pub fn new(
        orderbook: OrderBook,
        submit_receiver: SequencerReceiver,
        match_result_sender: MatchResultSender,
    ) -> Self {
        ApplyThread {
            orderbook,
            submit_receiver,
            match_result_sender,
        }
    }

    pub async fn run(mut self) {
        info!("ApplyThread started");
        loop {
            let cmd_wrapper = self.submit_receiver.recv().await;
            if cmd_wrapper.is_none() {
                break;
            }
            let cmd_wrapper = cmd_wrapper.expect("recv cmd_wrapper");
            match cmd_wrapper.inner {
                MatchCmd::MatchReq(req) => {
                    self.handle_match_req(req).await;
                }
                MatchCmd::MatchAdminCmd(_admin_cmd) => {}
            }
        }
        info!("ApplyThread stopped");
    }

    #[instrument(level = "info", skip_all)]
    pub async fn handle_match_req(&mut self, batch_match_req: oms::BatchMatchRequest) {
        let mut match_result_buffer: Vec<MatchResult> = Vec::new();
        for cmd in batch_match_req.cmds {
            let res = self.orderbook.process_trade_cmd(cmd);
            match res {
                Ok(result) => {
                    match_result_buffer.push(result);
                }
                Err(e) => {
                    info!("Error processing match request: {:?}", e);
                }
            }
        }
    }
}

struct MatchResultProducer {
    producer_cfg: ProducerConfig,
    producer: rdkafka::producer::FutureProducer,
    match_result_receiver: MatchResultReceiver,
}

impl MatchResultProducer {
    pub async fn run(mut self) {
        let thread_name = format!(
            "MatchResultProducer_{}",
            self.producer_cfg.trade_pair.pair()
        );
        info!("{} started", &thread_name);
        while let Some(cmd_wrapper) = self.match_result_receiver.recv().await {
            let batch_match_result = cmd_wrapper.inner;
            self.send(batch_match_result).await;
        }
        info!("{} stopped", &thread_name);
    }

    #[instrument(level = "info", skip_all)]
    async fn send(&self, batch_match_result: oms::BatchMatchResult) {
        let key = format!("match_result_{}", self.producer_cfg.trade_pair.pair());
        let buf = &BatchMatchResultTransfer::serialize(&batch_match_result)
            .expect("serialize match_result");
        self.producer
            .send(
                rdkafka::producer::FutureRecord::to(self.producer_cfg.topic())
                    .payload(buf)
                    .key(&key),
                std::time::Duration::from_millis(500),
            )
            .await
            .expect("send match result");
    }
}

struct MatchResultProducerBuilder<'a> {
    cfg: Option<&'a ProducerConfig>,
    match_result_receiver: Option<MatchResultReceiver>,
}

impl<'a> MatchResultProducerBuilder<'a> {
    pub fn new() -> Self {
        MatchResultProducerBuilder {
            cfg: None,
            match_result_receiver: None,
        }
    }

    pub fn with_producer_cfg(mut self, cfg: &'a ProducerConfig) -> Self {
        self.cfg = Some(cfg);
        self
    }

    pub fn with_match_result_receiver(mut self, receiver: MatchResultReceiver) -> Self {
        self.match_result_receiver = Some(receiver);
        self
    }

    pub fn build(mut self) -> Result<MatchResultProducer, Box<dyn std::error::Error>> {
        let cfg = self.cfg.expect("producer config");
        let producer = cfg.create_producer()?;
        Ok(MatchResultProducer {
            producer_cfg: cfg.clone(),
            producer,
            match_result_receiver: self
                .match_result_receiver
                .take()
                .expect("match result receiver"),
        })
    }
}
