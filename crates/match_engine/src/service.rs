//#![allow(dead_code)]
#![deny(clippy::unwrap_used)]

use std::{collections::HashMap, path::PathBuf};

use rdkafka::Message;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, info, instrument};

use tte_core::{
    pbcode::oms::{self},
    types::{BatchMatchReqTransfer, BatchMatchResultTransfer, TradePair},
};

use crate::{
    egress::AllowAllEgress,
    orderbook::{OrderBook, OrderBookErr},
    types::{CmdWrapper, MatchCmd, MatchCmdOutput, MatchResultReceiver, SequencerSender},
};
use tte_infra::kafka::{ConsumerConfig, ProducerConfig};
use tte_sequencer::raft::RaftSequencer;
use tte_sequencer::raft::{RaftSequencerBuilder, RaftSequencerConfig};

#[derive(Clone)]
pub struct MatchEngineService {
    #[allow(dead_code)]
    submit_sender: SequencerSender, // todo: admin cmd
}

unsafe impl Send for MatchEngineService {}
unsafe impl Sync for MatchEngineService {}

impl MatchEngineService {
    pub async fn build_component(
        raft_config: RaftSequencerConfig,
        trade_pair: TradePair,
        orderbook: OrderBook, // todo: 从DB加载快照
        producer_cfgs: HashMap<String, ProducerConfig>,
        consumer_cfgs: HashMap<String, ConsumerConfig>,
    ) -> Result<
        (
            SequencerSender,
            MatchReqConsumer,
            RaftSequencer<
                OrderBook,
                CmdWrapper<MatchCmd>,
                CmdWrapper<MatchCmdOutput>,
                AllowAllEgress,
            >,
            MatchResultProducer,
        ),
        OrderBookErr,
    > {
        let batch_size = 32;
        let req_topic = format!("match_req_{}{}", trade_pair.base, trade_pair.quote);
        let result_topic = format!("match_result_{}{}", trade_pair.base, trade_pair.quote);
        let req_consumer_cfg = consumer_cfgs
            .get(&req_topic)
            .expect("missing match-engine consumer config");
        let result_producer_cfg = producer_cfgs
            .get(&result_topic)
            .expect("missing match-engine producer config");

        let (match_result_sender, match_result_receiver) =
            mpsc::channel::<oms::BatchMatchResult>(batch_size);
        let (req_send, req_recv) = tokio::sync::mpsc::channel::<CmdWrapper<MatchCmd>>(batch_size);

        let sequencer: RaftSequencer<
            OrderBook,
            CmdWrapper<MatchCmd>,
            CmdWrapper<MatchCmdOutput>,
            AllowAllEgress,
        > = RaftSequencerBuilder::new()
            .with_node_id(*raft_config.node_id())
            .with_db_path(PathBuf::from(raft_config.db_path()))
            .with_nodes(raft_config.nodes().clone())
            .with_request_receiver(req_recv)
            .with_state_machine(orderbook)
            .with_egress(AllowAllEgress::new(match_result_sender))
            .build()
            .await
            .expect("build sequencer");

        let match_result_producer = MatchResultProducerBuilder::new()
            .with_producer_cfg(result_producer_cfg)
            .with_match_result_receiver(match_result_receiver)
            .with_node_id(raft_config.node_id().clone())
            .build()
            .expect("create producer");

        let match_req_consumer = MatchConsumerBuilder::new()
            .with_consumer_cfg(req_consumer_cfg)
            .with_sequencer_sender(&req_send)
            .build()
            .expect("create consumer");

        Ok((
            req_send,
            match_req_consumer,
            sequencer,
            match_result_producer,
        ))
    }

    pub async fn run_match_engine(
        raft_config: RaftSequencerConfig,
        trade_pair: TradePair,
        orderbook: OrderBook, // todo: 从DB加载快照
        producer_cfgs: HashMap<String, ProducerConfig>,
        consumer_cfgs: HashMap<String, ConsumerConfig>,
    ) -> Result<(Self, Vec<JoinHandle<()>>), OrderBookErr> {
        let (req_send, match_req_consumer, mut match_engine, match_result_producer) =
            Self::build_component(
                raft_config,
                trade_pair,
                orderbook,
                producer_cfgs,
                consumer_cfgs,
            )
            .await?;
        match_engine
            .load_last_seq_id()
            .await
            .expect("load seq_id from disk");

        let handlers = vec![
            tokio::spawn(async move {
                match_req_consumer.run().await;
            }),
            tokio::spawn(async move {
                match_engine.run().await;
            }),
            tokio::spawn(async move {
                match_result_producer.run().await;
            }),
        ];

        // init Sequencer, ApplyThread, MatchResultProducer, ConsumerTask
        Ok((
            MatchEngineService {
                submit_sender: req_send,
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

pub struct MatchReqConsumer {
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

        debug!(
            "Sending match request to sequencer, payload={}",
            serde_json::to_string(cmd_wrapper.inner()).unwrap()
        );

        self.sequencer_sender
            .send(cmd_wrapper)
            .await
            .expect("send to sequencer");
    }
}

// struct OrderBookService {
//     orderbook: OrderBook,
//     submit_receiver: SequencerReceiver,
//     match_result_sender: MatchResultSender,
// }

// #[deprecated]
// struct ApplyThread {
//     orderbook: OrderBook,
//     submit_receiver: SequencerReceiver,
//     match_result_sender: MatchResultSender,
// }

// impl ApplyThread {
//     pub fn new(
//         orderbook: OrderBook,
//         submit_receiver: SequencerReceiver,
//         match_result_sender: MatchResultSender,
//     ) -> Self {
//         ApplyThread {
//             orderbook,
//             submit_receiver,
//             match_result_sender,
//         }
//     }

//     pub async fn run(mut self) {
//         info!("ApplyThread started");
//         loop {
//             let cmd_wrapper = self.submit_receiver.recv().await;
//             if cmd_wrapper.is_none() {
//                 break;
//             }
//             let cmd_wrapper = cmd_wrapper.expect("recv cmd_wrapper");
//             self.orderbook.update_seq_id(cmd_wrapper.seq_id);
//             match cmd_wrapper.inner {
//                 MatchCmd::MatchReq(req) => {
//                     self.handle_match_req(req).await;
//                 }
//                 MatchCmd::MatchAdminCmd(admin_cmd) => match admin_cmd.admin_action {
//                     x if x == oms::AdminAction::TakeSnapshot as i32 => {
//                         info!(
//                             "TakeSnapshot admin command received, current seq_id: {}",
//                             cmd_wrapper.seq_id
//                         );

//                         match self.orderbook.take_snapshot() {
//                             Ok(snapshot) => {
//                                 self.persist_snapshot_json(snapshot).await;
//                             }
//                             Err(e) => {
//                                 error!("TakeSnapshot failed: {}", e);
//                             }
//                         }
//                     }
//                     _ => {}
//                 },
//             }
//         }
//         info!("ApplyThread stopped");
//     }

//     #[instrument(level = "info", skip_all)]
//     pub async fn handle_match_req(&mut self, batch_match_req: oms::BatchMatchRequest) {
//         let mut match_result_buffer = Vec::with_capacity(batch_match_req.cmds.len());
//         for cmd in batch_match_req.cmds {
//             let trade_id = cmd.trade_id;
//             let prev_trade_id = cmd.prev_trade_id;
//             let action =
//                 oms::BizAction::from_i32(cmd.rpc_cmd.as_ref().unwrap().biz_action).unwrap(); // refactor
//             let result = self.orderbook.process_trade_cmd(cmd);
//             match result {
//                 Ok(r) => match r.action {
//                     oms::BizAction::FillOrder => {
//                         let fill_result = r.fill_result.as_ref().unwrap();
//                         let match_result =
//                             MatchResult::into_fill_result_pb(trade_id, prev_trade_id, fill_result);
//                         match_result_buffer.push(match_result);
//                     }
//                     oms::BizAction::CancelOrder => {
//                         match_result_buffer.push(MatchResult::into_cancel_result_pb(
//                             trade_id,
//                             prev_trade_id,
//                             r.cancel_result.as_ref().unwrap(),
//                         ));
//                     }
//                     _ => {}
//                 },
//                 Err(e) => match_result_buffer.push(MatchResult::into_err(
//                     trade_id,
//                     prev_trade_id,
//                     action,
//                     e.to_string(),
//                 )),
//             }
//         }
//         // note: 基本上不可能; 即使发生可以通过重放恢复
//         if let Err(e) = self
//             .match_result_sender
//             .send(CmdWrapper::new(oms::BatchMatchResult {
//                 trade_pair: batch_match_req.trade_pair.clone(),
//                 results: match_result_buffer,
//             }))
//             .await
//         {
//             error!("send match result error: {}", e);
//         }
//     }

//     async fn persist_snapshot_json(&self, snapshot: OrderBookSnapshot) {
//         let filename = format!(
//             "./snapshot/orderbook_snapshot_{}_{}_{}.json",
//             snapshot.trade_pair().pair(),
//             snapshot.id_manager().seq_id(),
//             chrono::Utc::now().timestamp_millis() as u64,
//         );
//         match serde_json::to_string_pretty(&snapshot) {
//             Err(e) => {
//                 error!("serialize snapshot to json failed: {}", e);
//                 return;
//             }
//             Ok(json) => {
//                 tokio::fs::write(&filename, json).await.unwrap();
//                 info!("persisted snapshot to file: {}", filename);
//             }
//         }
//     }
// }

pub struct MatchResultProducer {
    producer_cfg: ProducerConfig,
    producer: rdkafka::producer::FutureProducer,
    match_result_receiver: MatchResultReceiver,
    msg_headers: rdkafka::message::OwnedHeaders,
}

impl MatchResultProducer {
    pub async fn run(mut self) {
        let thread_name = format!(
            "MatchResultProducer_{}",
            self.producer_cfg.trade_pair.pair()
        );
        info!("{} started", &thread_name);
        while let Some(batch_match_result) = self.match_result_receiver.recv().await {
            self.send(batch_match_result).await;
        }
        info!("{} stopped", &thread_name);
    }

    #[instrument(level = "info", skip_all)]
    async fn send(&self, batch_match_result: oms::BatchMatchResult) {
        let key = format!("match_result_{}", self.producer_cfg.trade_pair.pair());
        let buf = &BatchMatchResultTransfer::serialize(&batch_match_result)
            .expect("serialize match_result");

        debug!(
            "match result: {}",
            serde_json::to_string(&batch_match_result).unwrap()
        );

        self.producer
            .send(
                rdkafka::producer::FutureRecord::to(self.producer_cfg.topic())
                    .headers(self.msg_headers.clone())
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
    node_id: Option<u64>,
}

impl<'a> MatchResultProducerBuilder<'a> {
    pub fn new() -> Self {
        MatchResultProducerBuilder {
            cfg: None,
            match_result_receiver: None,
            node_id: None,
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

    pub fn with_node_id(mut self, node_id: u64) -> Self {
        self.node_id = Some(node_id);
        self
    }

    pub fn build(mut self) -> Result<MatchResultProducer, Box<dyn std::error::Error>> {
        let cfg = self.cfg.expect("producer config");
        let producer = cfg.create_producer()?;

        let headers = rdkafka::message::OwnedHeaders::new()
            .insert(rdkafka::message::Header {
                key: "source",
                value: Some("match_engine"),
            })
            .insert(rdkafka::message::Header {
                key: "raft_node_id",
                value: Some(&self.node_id.expect("node_id").to_string()),
            });

        Ok(MatchResultProducer {
            msg_headers: headers,
            producer_cfg: cfg.clone(),
            producer,
            match_result_receiver: self
                .match_result_receiver
                .take()
                .expect("match result receiver"),
        })
    }
}
