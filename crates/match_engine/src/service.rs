//#![allow(dead_code)]
#![deny(clippy::unwrap_used)]

use std::collections::HashMap;

use rdkafka::Message;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{error, info, instrument};

use tte_core::{
    pbcode::oms::{self},
    types::{BatchMatchReqTransfer, BatchMatchResultTransfer, MatchResult, TradePair},
};

use crate::orderbook::{OrderBook, OrderBookErr, OrderBookSnapshot, OrderBookSnapshotHandler};
use tte_infra::kafka::{ConsumerConfig, ProducerConfig, print_kafka_msg_meta};
use tte_sequencer::api::{DefaultSequencer, SequenceSetter};

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

type SequencerSender = tokio::sync::mpsc::Sender<CmdWrapper<MatchCmd>>;
type SequencerReceiver = tokio::sync::mpsc::Receiver<CmdWrapper<MatchCmd>>;
type MatchResultSender = tokio::sync::mpsc::Sender<CmdWrapper<oms::BatchMatchResult>>;
type MatchResultReceiver = tokio::sync::mpsc::Receiver<CmdWrapper<oms::BatchMatchResult>>;

#[derive(Clone)]
pub struct MatchEngineService {
    #[allow(dead_code)]
    submit_sender: SequencerSender, // todo: admin cmd
}

unsafe impl Send for MatchEngineService {}
unsafe impl Sync for MatchEngineService {}

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

#[tonic::async_trait]
impl oms::match_engine_service_server::MatchEngineService for MatchEngineService {
    // 内部使用
    #[instrument(level = "info", skip_all)]
    async fn take_snapshot(
        &self,
        _request: tonic::Request<oms::TakeSnapshotReq>,
    ) -> std::result::Result<tonic::Response<oms::TakeSnapshotRsp>, tonic::Status> {
        self.submit_sender
            .send_timeout(
                CmdWrapper::new(MatchCmd::MatchAdminCmd(oms::MatchAdminCmd {
                    admin_action: oms::AdminAction::TakeSnapshot as i32,
                })),
                std::time::Duration::from_secs(1),
            )
            .await
            .map_err(|e| {
                tonic::Status::internal(format!("failed to send take_snapshot command: {}", e))
            })?;
        Ok(tonic::Response::new(oms::TakeSnapshotRsp {}))
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
            self.orderbook.update_seq_id(cmd_wrapper.seq_id);
            match cmd_wrapper.inner {
                MatchCmd::MatchReq(req) => {
                    self.handle_match_req(req).await;
                }
                MatchCmd::MatchAdminCmd(admin_cmd) => match admin_cmd.admin_action {
                    x if x == oms::AdminAction::TakeSnapshot as i32 => {
                        info!(
                            "TakeSnapshot admin command received, current seq_id: {}",
                            cmd_wrapper.seq_id
                        );

                        match self.orderbook.take_snapshot() {
                            Ok(snapshot) => {
                                self.persist_snapshot_json(snapshot).await;
                            }
                            Err(e) => {
                                error!("TakeSnapshot failed: {}", e);
                            }
                        }
                    }
                    _ => {}
                },
            }
        }
        info!("ApplyThread stopped");
    }

    #[instrument(level = "info", skip_all)]
    pub async fn handle_match_req(&mut self, batch_match_req: oms::BatchMatchRequest) {
        let mut match_result_buffer = Vec::with_capacity(batch_match_req.cmds.len());
        for cmd in batch_match_req.cmds {
            let trade_id = cmd.trade_id;
            let prev_trade_id = cmd.prev_trade_id;
            let action =
                oms::BizAction::from_i32(cmd.rpc_cmd.as_ref().unwrap().biz_action).unwrap(); // refactor
            match self.orderbook.process_trade_cmd(cmd) {
                Ok(r) => match r.action {
                    oms::BizAction::FillOrder => {
                        let fill_result = r.fill_result.as_ref().unwrap();
                        let match_result =
                            MatchResult::into_fill_result_pb(trade_id, prev_trade_id, fill_result);
                        match_result_buffer.push(match_result);
                    }
                    oms::BizAction::CancelOrder => {
                        match_result_buffer.push(MatchResult::into_cancel_result_pb(
                            trade_id,
                            prev_trade_id,
                            r.cancel_result.as_ref().unwrap(),
                        ));
                    }
                    _ => {}
                },
                Err(e) => match_result_buffer.push(MatchResult::into_err(
                    trade_id,
                    prev_trade_id,
                    action,
                    e.to_string(),
                )),
            }
        }
        // note: 基本上不可能; 即使发生可以通过重放恢复
        if let Err(e) = self
            .match_result_sender
            .send(CmdWrapper::new(oms::BatchMatchResult {
                trade_pair: batch_match_req.trade_pair.clone(),
                results: match_result_buffer,
            }))
            .await
        {
            error!("send match result error: {}", e);
        }
    }

    async fn persist_snapshot_json(&self, snapshot: OrderBookSnapshot) {
        let filename = format!(
            "./snapshot/orderbook_snapshot_{}_{}_{}.json",
            snapshot.trade_pair().pair(),
            snapshot.id_manager().seq_id(),
            chrono::Utc::now().timestamp_millis() as u64,
        );
        match serde_json::to_string_pretty(&snapshot) {
            Err(e) => {
                error!("serialize snapshot to json failed: {}", e);
                return;
            }
            Ok(json) => {
                tokio::fs::write(&filename, json).await.unwrap();
                info!("persisted snapshot to file: {}", filename);
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
