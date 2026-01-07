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
use tte_rlr::{AppStateMachineHandler, RaftService, Rlr};

use crate::{
    egress::AllowAllEgress,
    orderbook::{OrderBook, OrderBookErr, OrderBookSnapshotHandler},
    state_machine::OrderBookBizViewBuilder,
    types::{CmdWrapper, MatchCmd, MatchCmdOutput, MatchResultReceiver, SequencerSender},
};
use tte_infra::kafka::{ConsumerConfig, ProducerConfig};
use tte_sequencer::raft::RaftSequencer;
use tte_sequencer::raft::{RaftSequencerBuilder, RaftSequencerConfig};

#[derive(Clone)]
pub struct MatchEngineService {
    raft_view: Rlr,
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
            RaftService,
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
            .with_snapshot_path(PathBuf::from(raft_config.snapshot_path()))
            .with_nodes(raft_config.nodes().clone())
            .with_request_receiver(req_recv)
            .with_state_machine(orderbook)
            .with_egress(AllowAllEgress::new(match_result_sender))
            .with_raft_config(tte_rlr::Config {
                heartbeat_interval: 500,
                election_timeout_min: 1500,
                election_timeout_max: 3000,
                max_payload_entries: 1024,
                snapshot_policy: tte_rlr::SnapshotPolicy::LogsSinceLast(10000),
                ..Default::default()
            })
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

        let raft_node = sequencer.raft().clone();
        Ok((
            req_send,
            match_req_consumer,
            sequencer,
            RaftService::new(raft_node),
            match_result_producer,
        ))
    }

    pub async fn run_match_engine(
        raft_config: RaftSequencerConfig,
        trade_pair: TradePair,
        orderbook: OrderBook, // todo: 从DB加载快照
        producer_cfgs: HashMap<String, ProducerConfig>,
        consumer_cfgs: HashMap<String, ConsumerConfig>,
    ) -> Result<(Self, RaftService, Vec<JoinHandle<()>>), OrderBookErr> {
        let (req_send, match_req_consumer, mut match_engine, raft_service, match_result_producer) =
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
                if let Err(e) = match_engine.run().await {
                    tracing::error!("Match engine error: {}", e);
                }
            }),
            tokio::spawn(async move {
                match_result_producer.run().await;
            }),
        ];

        // init Sequencer, ApplyThread, MatchResultProducer, ConsumerTask
        // let me_read_view = raft_service.clone();
        Ok((
            MatchEngineService {
                raft_view: raft_service.raft().clone(),
                submit_sender: req_send,
            },
            raft_service,
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
        let (sender, recv) = tokio::sync::oneshot::channel();
        match self
            .raft_view
            .with_state_machine(|sm: &mut AppStateMachineHandler<OrderBook>| {
                Box::pin(async {
                    let view = sm.read_state();
                    match view.read().await.take_biz_snapshot() {
                        Ok(snapshot) => {
                            let _ = sender.send(snapshot);
                        }
                        Err(e) => {
                            tracing::error!("Failed to take snapshot: {}", e);
                        }
                    };
                })
            })
            .await
        {
            Err(e) => {
                tracing::error!("Failed to take snapshot: {}", e);
                return Err(tonic::Status::internal("Failed to take snapshot"));
            }
            Ok(_) => {
                tracing::info!("Snapshot taken successfully.");
            }
        };

        let snapshot = recv.await.expect("receive snapshot");
        let builder = OrderBookBizViewBuilder::new();
        match builder.persist_snapshot_json(snapshot).await {
            Ok(_) => {
                tracing::info!("Snapshot persisted successfully.");
                Ok(tonic::Response::new(oms::TakeSnapshotRsp {}))
            }
            Err(e) => {
                tracing::error!("Failed to persist snapshot: {}", e);
                Err(tonic::Status::internal("Failed to persist snapshot"))
            }
        }
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

        // key:string, value:string
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
