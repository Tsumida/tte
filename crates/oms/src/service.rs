#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::OMSErr;
use crate::oms::{OMS, OrderBuilder};
use crate::oms::{OMSMatchResultHandler, OMSRpcHandler};
use futures::StreamExt;
use prost::Message as _;
use rdkafka::Message;
use rdkafka::message::BorrowedMessage;
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{error, info, instrument, warn};
use tte_core::pbcode::oms;
use tte_core::types::{BatchMatchResultTransfer, TradePair};
use tte_core::{err_code, types};
use tte_infra::kafka::{ConsumerConfig, ProducerConfig, print_kafka_msg_meta};
use tte_sequencer::api::{DefaultSequencer, SequenceSetter};

type InformSender = oneshot::Sender<Informer>;
type InformReceiver = oneshot::Receiver<Informer>;

#[derive(Debug, Clone)]
struct Informer {
    seq_id: u64,
    prev_seq_id: u64,
    is_success: bool,
    err: Option<OMSErr>,
}

type MatchReqSender = mpsc::Sender<oms::BatchMatchRequest>;
type MatchReqReceiver = mpsc::Receiver<oms::BatchMatchRequest>;

fn match_req_chan(chan_size: usize) -> (MatchReqSender, MatchReqReceiver) {
    mpsc::channel::<oms::BatchMatchRequest>(chan_size)
}

pub trait OrderRouter {
    fn route_key(&self) -> Option<&oms::TradePair>;
}

#[derive(Debug, Clone)]
enum CmdFlow {
    TradeCmd(oms::TradeCmd),
    MatchResult(oms::BatchMatchResult),
}

// refactor: 这个类型字段起来很麻烦;
#[derive(Debug)]
struct OMSCmd {
    // cmd: Arc<Box<oms::TradeCmd>>,
    seq_id: u64,
    prev_seq_id: u64,
    cmd: CmdFlow,
    rsp_chan: Option<InformSender>,
}

impl SequenceSetter for OMSCmd {
    fn set_seq_id(&mut self, seq_id: u64, prev_seq_id: u64) {
        self.seq_id = seq_id;
        self.prev_seq_id = prev_seq_id;
    }
}

impl OMSCmd {
    fn place_order_cmd(req: oms::PlaceOrderReq) -> (Self, InformReceiver) {
        let (rsp_chan, rsp_recv) = oneshot::channel();
        // refactor: avoid unwrap
        let trade_pair = req.order.as_ref().unwrap().trade_pair.as_ref().unwrap();
        (
            OMSCmd {
                seq_id: 0,
                prev_seq_id: 0,
                cmd: CmdFlow::TradeCmd(oms::TradeCmd {
                    trade_id: 0,
                    prev_trade_id: 0,
                    trade_pair: Some(trade_pair.clone()),
                    rpc_cmd: Some(oms::RpcCmd {
                        biz_action: oms::BizAction::PlaceOrder as i32,
                        place_order_req: Some(req),
                        cancel_order_req: None,
                    }),
                }),
                rsp_chan: Some(rsp_chan),
            },
            rsp_recv,
        )
    }

    fn cancel_order_cmd(req: oms::CancelOrderReq) -> (Self, InformReceiver) {
        let (rsp_chan, rsp_recv) = oneshot::channel();
        let trade_pair = TradePair::new(&req.base, &req.quote);
        (
            OMSCmd {
                seq_id: 0,
                prev_seq_id: 0,
                cmd: CmdFlow::TradeCmd(oms::TradeCmd {
                    trade_id: 0,
                    prev_trade_id: 0,
                    trade_pair: Some(trade_pair.clone()),
                    rpc_cmd: Some(oms::RpcCmd {
                        biz_action: oms::BizAction::CancelOrder as i32,
                        place_order_req: None,
                        cancel_order_req: Some(req),
                    }),
                }),
                rsp_chan: Some(rsp_chan),
            },
            rsp_recv,
        )
    }

    fn match_result_cmd(match_results: oms::BatchMatchResult) -> Self {
        OMSCmd {
            seq_id: 0,
            prev_seq_id: 0,
            cmd: CmdFlow::MatchResult(match_results),
            rsp_chan: None,
        }
    }
}

#[derive(Debug)]
pub struct TradeSystem {
    oms_view: Arc<RwLock<OMS>>, // read only
    submit_sender: mpsc::Sender<OMSCmd>,
}

impl TradeSystem {
    pub async fn run_trade_system(
        oms: OMS,
        producer_cfgs: HashMap<TradePair, ProducerConfig>,
        consumer_cfgs: HashMap<TradePair, ConsumerConfig>,
    ) -> Result<(Self, Vec<tokio::task::JoinHandle<()>>), Box<dyn std::error::Error>> {
        let init_seq_id = oms.seq_id();
        let chan_size = 128;

        let (sequencer, submit_send, commit_recv) =
            DefaultSequencer::<OMSCmd>::new(init_seq_id, chan_size);
        let shared_oms = Arc::new(RwLock::new(oms));
        let (match_req_sender, match_req_receiver) = match_req_chan(chan_size);

        let mut apply_thread = ApplyThreadBuilder::new(shared_oms.clone())
            .with_commit_receiver(commit_recv)
            .with_match_req_sender(match_req_sender)
            .build()
            .expect("ApplyThread build success");

        // todo: panic if dependencies fail
        // todo: graceful shutdown
        let svc = Self {
            oms_view: shared_oms,
            submit_sender: submit_send.clone(),
        };
        let mut match_req_thread = MatchRequestSender::new(
            match_req_receiver,
            producer_cfgs, // todo 提取
        )
        .init()
        .await
        .expect("init success");

        let match_result_consumer = MatchResultConsumer::new(consumer_cfgs)
            .init()
            .await
            .expect("match_result_consumer init");

        let handlers = vec![
            // sequencer thread
            tokio::spawn(async move {
                sequencer.run().await;
            }),
            // apply & response thread
            tokio::spawn(async move {
                apply_thread.run().await;
            }),
            // match request sender
            tokio::spawn(async move { match_req_thread.run().await }),
            //
            tokio::spawn(async move { match_result_consumer.run(submit_send.clone()).await }),
        ];

        Ok((svc, handlers))
    }

    async fn propose(&self, cmd: OMSCmd) -> Result<(), OMSErr> {
        self.submit_sender.send(cmd).await.map_err(|e| {
            tracing::error!("Failed to propose cmd: {:?}", e);
            OMSErr::new(err_code::ERR_INTERNAL, "sequencer failed")
        })
    }
}

// Impl RPC Handler
#[tonic::async_trait]
impl oms::oms_service_server::OmsService for TradeSystem {
    #[instrument(level = "info", skip_all)]
    async fn place_order(
        &self,
        req: tonic::Request<oms::PlaceOrderReq>,
    ) -> Result<tonic::Response<oms::PlaceOrderRsp>, tonic::Status> {
        let place_order_req = req.get_ref();
        if place_order_req.order.is_none() {
            return Err(tonic::Status::invalid_argument("Order detail is missing"));
        }

        let order = place_order_req.order.as_ref().unwrap();
        let order = OrderBuilder::new()
            .build(0, 0, &order) // note: trade_id=0
            .map_err(|e| tonic::Status::invalid_argument(format!("Invalid order detail: {}", e)))?;

        let oms = self.oms_view.read().await;
        _ = oms.check_place_order(&order).map_err(|e| {
            tracing::warn!("Failed to check place order: {}", e);
            tonic::Status::failed_precondition(format!("Precondition failed: {}", e))
        })?;
        drop(oms); // critical: avoid deadlock

        // todo, write order into sequencer channel and wait for response.
        let (cmd, rsp_recv) = OMSCmd::place_order_cmd(place_order_req.clone());
        let start_time = std::time::Instant::now();
        let _ = self
            .propose(cmd)
            .await
            .map_err(|e| tonic::Status::internal(format!("Sequencer append failed: {:?}", e)))?;

        // todo: timeout
        let _ = rsp_recv
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to receive response: {:?}", e)))?;

        tracing::info!(
            "PlaceOrder done, dur={} ms",
            start_time.elapsed().as_millis(),
        );

        Ok(tonic::Response::new(oms::PlaceOrderRsp {}))
    }

    #[instrument(level = "info", skip_all)]
    async fn cancel_order(
        &self,
        req: tonic::Request<oms::CancelOrderReq>,
    ) -> Result<tonic::Response<oms::CancelOrderRsp>, tonic::Status> {
        let cancel_order_req = req.get_ref();
        if cancel_order_req.order_id.is_empty() {
            return Err(tonic::Status::invalid_argument("Order ID is missing"));
        }

        let oms = self.oms_view.read().await;
        let order = oms
            .get_order_detail(cancel_order_req.account_id, &cancel_order_req.order_id)
            .ok_or_else(|| tonic::Status::not_found("Order not found"))?;

        match order.current_state() {
            // ideompotent
            types::OrderState::Cancelled => {
                return Ok(tonic::Response::new(oms::CancelOrderRsp {}));
            }
            types::OrderState::PartiallyFilled
            | types::OrderState::New
            | types::OrderState::PendingNew => {
                // 可以撤单，但最终结果还是要看撮合结果
                tracing::info!(
                    "allow cancel order {}, current state={:?}",
                    cancel_order_req.order_id,
                    order.current_state(),
                );
            }
            _ => {
                return Err(tonic::Status::failed_precondition(
                    "Invalid order state for cancel op",
                ));
            }
        }
        drop(oms); // critical: avoid deadlock

        let (cmd, rsp_recv) = OMSCmd::cancel_order_cmd(cancel_order_req.clone());
        let start_time = std::time::Instant::now();
        let _ =
            self.submit_sender.send(cmd).await.map_err(|e| {
                tonic::Status::internal(format!("Sequencer append failed: {:?}", e))
            })?;

        let _ = rsp_recv
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to receive response: {:?}", e)))?;

        tracing::info!(
            "CancelOrder done, dur={} ms",
            start_time.elapsed().as_millis(),
        );

        Ok(tonic::Response::new(oms::CancelOrderRsp {}))
    }

    #[instrument(level = "info", skip_all)]
    async fn get_order_detail(
        &self,
        req: tonic::Request<oms::GetOrderDetailReq>,
    ) -> Result<tonic::Response<oms::GetOrderDetailRsp>, tonic::Status> {
        let request = req.get_ref();
        let order_id = &request.order_id;
        let account_id = request.account_id;
        let view = self.oms_view.read().await;

        if !order_id.is_empty() {
            let order = view
                .get_order_detail(account_id, order_id)
                .ok_or_else(|| tonic::Status::not_found("order not found by order_id"))?;
            Ok(tonic::Response::new(oms::GetOrderDetailRsp {
                detail: Some(order.into()),
            }))
        } else {
            let a = view
                .get_order_detail_by_client_id(account_id, &request.client_order_id)
                .ok_or_else(|| tonic::Status::not_found("order not found by client_order_id"))?;
            return Ok(tonic::Response::new(oms::GetOrderDetailRsp {
                detail: Some(a.into()),
            }));
        }
    }

    #[instrument(level = "info", skip_all)]
    async fn get_balance(
        &self,
        req: tonic::Request<oms::GetBalanceReq>,
    ) -> Result<tonic::Response<oms::GetBalanceRsp>, tonic::Status> {
        let request = req.get_ref();
        let account_id = request.account_id;
        let view = self.oms_view.read().await;

        let balance = view.get_ledger().get_balance(account_id);
        Ok(tonic::Response::new(oms::GetBalanceRsp {
            account_id,
            balances: balance
                .into_iter()
                .map(
                    |(currency, deposit, frozen, update_time)| oms::BalanceItem {
                        currency,
                        balance: (deposit + frozen).to_string(),
                        available: deposit.to_string(),
                        frozen: frozen.to_string(),
                        update_time,
                    },
                )
                .collect(),
        }))
    }

    #[instrument(level = "info", skip_all)]
    async fn transfer_freeze(
        &self,
        _req: tonic::Request<oms::TransferFreezeReq>,
    ) -> Result<tonic::Response<oms::TransferFreezeRsp>, tonic::Status> {
        todo!()
    }

    #[instrument(level = "info", skip_all)]
    async fn transfer(
        &self,
        _req: tonic::Request<oms::TransferReq>,
    ) -> Result<tonic::Response<oms::TransferRsp>, tonic::Status> {
        todo!()
    }

    #[instrument(level = "info", skip_all)]
    async fn take_snapshot(
        &self,
        _: tonic::Request<oms::TakeSnapshotReq>,
    ) -> Result<tonic::Response<oms::TakeSnapshotRsp>, tonic::Status> {
        // save at oms_snapshot_{.tiomestamp}.json
        let snapshot = self.oms_view.read().await.take_snapshot();

        serde_json::to_writer_pretty(
            std::fs::File::create(format!(
                "./snapshot/oms_snapshot_{}_{}.json",
                snapshot.id_manager().seq_id(),
                snapshot.timestamp()
            ))
            .map_err(|e| {
                tonic::Status::internal(format!("Failed to create snapshot file: {:?}", e))
            })?,
            &snapshot,
        )
        .map_err(|e| tonic::Status::internal(format!("Failed to write snapshot file: {:?}", e)))?;

        Ok(tonic::Response::new(oms::TakeSnapshotRsp {}))
    }

    #[instrument(level = "info", skip_all)]
    async fn update_trade_pair_config(
        &self,
        _req: tonic::Request<oms::UpdateTradePairConfigReq>,
    ) -> Result<tonic::Response<oms::UpdateTradePairConfigRsp>, tonic::Status> {
        todo!()
    }
}

struct ReplayThread {
    reply_chan: oneshot::Sender<Informer>,
}

struct ApplyThreadBuilder {
    oms: Arc<RwLock<OMS>>,
    commit_recv: Option<mpsc::Receiver<OMSCmd>>,
    match_req_sender: Option<MatchReqSender>,
}

impl ApplyThreadBuilder {
    fn new(oms: Arc<RwLock<OMS>>) -> Self {
        Self {
            oms,
            commit_recv: None,
            match_req_sender: None,
        }
    }

    fn with_commit_receiver(mut self, recv: mpsc::Receiver<OMSCmd>) -> Self {
        self.commit_recv = Some(recv);
        self
    }

    fn with_match_req_sender(mut self, sender: MatchReqSender) -> Self {
        self.match_req_sender = Some(sender);
        self
    }

    fn build(self) -> Result<ApplyThread, &'static str> {
        if let Some(recv) = self.commit_recv {
            Ok(ApplyThread {
                oms: self.oms,
                commit_recv: recv,
                match_req_sender: self.match_req_sender.expect("match_req_sender"),
            })
        } else {
            Err("Missing commit receiver")
        }
    }
}

// 一次OMSCmd对OMS状态变更结果
struct ApplyThread {
    oms: Arc<RwLock<OMS>>,
    commit_recv: mpsc::Receiver<OMSCmd>,
    match_req_sender: MatchReqSender,
}

impl ApplyThread {
    async fn run(&mut self) {
        let batch_apply_size = 8;
        let mut batch = Vec::with_capacity(batch_apply_size);
        // todo: order events, ledger events

        tracing::info!("ApplyThread started");
        // todo: err handle for commit_recv
        while let Some(oms_cmd) = self.commit_recv.recv().await {
            let mut match_requests = Vec::with_capacity(batch_apply_size);

            batch.push(oms_cmd);
            while batch.len() < batch_apply_size {
                match self.commit_recv.try_recv() {
                    Ok(oms_cmd) => batch.push(oms_cmd),
                    Err(_) => break,
                }
            }

            for cmd in batch.drain(..) {
                // todo: set ready if prev_seq_id <= oms.seq_id, else waits preceding cmds
                match cmd.cmd {
                    CmdFlow::TradeCmd(trade_cmd) => {
                        // todo: validate cmd fields
                        let trade_pair = trade_cmd.trade_pair.as_ref().unwrap();
                        let mut oms = self.oms.write().await;
                        oms.id_manager.update_seq_id(cmd.seq_id);
                        match oms.handle_rpc_cmd(cmd.seq_id, trade_pair, trade_cmd.rpc_cmd.unwrap())
                        {
                            Ok(change_res) => {
                                if let Some(req) = change_res.match_request {
                                    match_requests.push(req);
                                }
                            }
                            Err(e) => {
                                error!(
                                    "OMS cmd(trade_id={}, prev={}) error: {:?}",
                                    cmd.seq_id, cmd.prev_seq_id, e
                                );
                            }
                        }
                    }
                    CmdFlow::MatchResult(batch_match_result) => {
                        let mut oms = self.oms.write().await;
                        for mr in batch_match_result.results.into_iter() {
                            if !mr.is_success {
                                error!(
                                    "OMS match_result(trade_id={}, prev={}) failed in ME",
                                    cmd.seq_id, cmd.prev_seq_id,
                                );
                                continue;
                            }
                            match Self::handle_match_result(&mut oms, &mr).await {
                                Ok(_) => {}
                                Err(e) => {
                                    error!(
                                        "OMS match_result(trade_id={}, prev={}) invalid: {:?}",
                                        cmd.seq_id, cmd.prev_seq_id, e
                                    );
                                }
                            }
                        }
                        drop(oms);
                    }
                }
                if let Some(ch) = cmd.rsp_chan {
                    if ch
                        .send(Informer {
                            seq_id: cmd.seq_id,
                            prev_seq_id: cmd.prev_seq_id,
                            is_success: true,
                            err: None,
                        })
                        .is_err()
                    {
                        warn!(
                            "ApplyThread: failed to send informer for seq_id={}",
                            cmd.seq_id
                        );
                    }
                }
            }

            if let Err(e) = self
                .send_match_requests(&self.match_req_sender, match_requests)
                .await
            {
                error!("Failed to send match requests: {:?}", e);
                break;
            }
            batch.clear();
        }

        tracing::info!("ApplyThread stopped");
    }

    // todo: order events
    async fn handle_match_result(oms: &mut OMS, mr: &oms::MatchResult) -> Result<(), OMSErr> {
        let action = oms::BizAction::from_i32(mr.action).ok_or_else(|| {
            OMSErr::new(
                err_code::ERR_OMS_INVALID_MATCH_RESULT,
                "invalid biz_action in match result",
            )
        })?;
        match action {
            oms::BizAction::FillOrder => {
                for record in &mr.records {
                    match oms.handle_success_fill(record) {
                        Ok(_change_res) => {
                            // note: 一般是没有的
                        }
                        Err(e) => {
                            error!(
                                "OMS match_result(match_id={}, prev_match_id={}) error: {:?}",
                                &record.match_id, &record.prev_match_id, e
                            );
                        }
                    };
                }
            }
            oms::BizAction::PlaceOrder => {
                // ignore
                info!("Ignore PlaceOrder match result");
            }
            oms::BizAction::CancelOrder => {
                if let Some(result) = mr.cancel_result.as_ref() {
                    oms.handle_success_cancel(result)?;
                } else {
                    return Err(OMSErr::new(
                        err_code::ERR_OMS_INVALID_MATCH_RESULT,
                        "missing cancel_result in match result",
                    ));
                }
            }
            _ => {
                return Err(OMSErr::new(
                    err_code::ERR_OMS_INVALID_MATCH_RESULT,
                    "unsupported biz_action in match result",
                ));
            }
        }
        Ok(())
    }

    async fn send_match_requests(
        &self,
        match_req_sender: &MatchReqSender,
        match_requests: Vec<oms::BatchMatchRequest>,
    ) -> Result<(), OMSErr> {
        for req in match_requests.into_iter() {
            match_req_sender.send(req).await.map_err(|e| {
                tracing::error!("Failed to send match request: {:?}", e);
                OMSErr::new(err_code::ERR_INTERNAL, "match request send failed")
            })?;
        }
        Ok(())
    }
}

struct MatchReqestProcesser {}

impl MatchReqestProcesser {
    // []CmdExt ->
    fn serialize(batch: oms::BatchMatchRequest) -> Vec<u8> {
        batch.encode_to_vec()
    }

    fn deserialize(payload: &[u8]) -> Result<oms::BatchMatchRequest, OMSErr> {
        match oms::BatchMatchRequest::decode(payload) {
            Ok(cmd) => Ok(cmd),
            Err(e) => {
                error!("Failed to deserialize BatchMatchRequest: {:?}", e);
                Err(OMSErr::new(
                    err_code::ERR_OMS_INVALID_MATCH_RESULT,
                    "invalid payload",
                ))
            }
        }
    }
}

// 撮合请求路由
// todo: TradePair -> ProducerConfig
// todo:
struct MatchRequestSender {
    receiver: MatchReqReceiver,
    configs: HashMap<TradePair, ProducerConfig>,
    routers: HashMap<String, MatchEngineRouter>, // key: TradePair.pair()
}

struct MatchEngineRouter {
    topic: String,
    trade_pair: TradePair,
    producers: rdkafka::producer::FutureProducer,
}

impl MatchRequestSender {
    fn new(receiver: MatchReqReceiver, configs: HashMap<TradePair, ProducerConfig>) -> Self {
        Self {
            receiver,
            configs,
            routers: HashMap::new(),
        }
    }

    async fn init(mut self) -> Result<Self, OMSErr> {
        for (pair, cfg) in self.configs.iter() {
            let producer = cfg.create_producer().map_err(|e| {
                tracing::error!("Failed to create Kafka producer: {:?}", e);
                OMSErr::new(err_code::ERR_INTERNAL, "kafka producer create failed")
            })?;
            self.routers.insert(
                pair.pair(),
                MatchEngineRouter {
                    topic: cfg.topic().clone(),
                    trade_pair: pair.clone(),
                    producers: producer,
                },
            );
        }
        Ok(self)
    }

    async fn run(&mut self) {
        tracing::info!("MatchRequesrSender start");
        while let Some(req) = self.receiver.recv().await {
            let trade_pair = req.trade_pair.as_ref().expect("trade pair field");
            if let Err(e) = self.route_match_req(trade_pair, req.cmds).await {
                error!(
                    "Failed to route match request for trade_pair={}: {:?}",
                    trade_pair.pair(),
                    e
                );
            }
        }
        info!("MatchRequestSender stopped");
    }

    #[instrument(
        level = "info", skip(self, cmds), 
        fields(
            trade_pair = %trade_pair.pair(),
        )
    )]
    async fn route_match_req(
        &self,
        trade_pair: &TradePair,
        cmds: Vec<oms::TradeCmd>,
    ) -> Result<(), OMSErr> {
        let pair = trade_pair.pair();
        if let Some(router) = self.routers.get(&pair) {
            let trade_ids: Vec<(u64, u64)> =
                cmds.iter().map(|c| (c.trade_id, c.prev_trade_id)).collect();

            let buf = MatchReqestProcesser::serialize(oms::BatchMatchRequest {
                trade_pair: Some(trade_pair.clone()),
                cmds,
            });

            tracing::info!("Routing to {}: trade_ids={:?}", pair, trade_ids);
            router
                .producers
                .send(
                    rdkafka::producer::FutureRecord::to(&router.topic)
                        .key(&pair)
                        .partition(0)
                        .payload(&buf), // todo: serialize
                    std::time::Duration::from_millis(500),
                )
                .await
                .map_err(|(e, _)| {
                    tracing::error!("Failed to send match request: {:?}", e);
                    OMSErr::new(err_code::ERR_INTERNAL, "kafka send failed")
                })?;
        } else {
            error!("No router found for trade pair: {}", pair);
            return Err(OMSErr::new(
                err_code::ERR_INTERNAL,
                "no router for trade pair",
            ));
        }
        Ok(())
    }
}

struct MatchResultConsumer {
    consumer_cfgs: HashMap<TradePair, ConsumerConfig>,
    consumers: HashMap<String, rdkafka::consumer::StreamConsumer>,
}

impl MatchResultConsumer {
    pub fn new(consumer_cfgs: HashMap<TradePair, ConsumerConfig>) -> Self {
        Self {
            consumer_cfgs,
            consumers: HashMap::new(),
        }
    }

    pub async fn init(mut self) -> Result<Self, OMSErr> {
        for (pair, cfg) in self.consumer_cfgs.iter_mut() {
            let pair_str = pair.pair();
            tracing::info!("MatchResultConsumer config ({}): {:?}", pair_str, cfg);
            self.consumers.insert(
                pair_str,
                cfg.subscribe().map_err(|e| {
                    tracing::error!("Failed to create Kafka consumer: {:?}", e);
                    OMSErr::new(err_code::ERR_INTERNAL, "kafka consumer create failed")
                })?,
            );
        }
        Ok(self)
    }

    async fn propose(submit_send: &mpsc::Sender<OMSCmd>, cmd: OMSCmd) -> Result<(), OMSErr> {
        submit_send.send(cmd).await.map_err(|e| {
            tracing::error!("Failed to propose match result cmd: {:?}", e);
            OMSErr::new(err_code::ERR_INTERNAL, "sequencer failed")
        })
    }

    pub async fn run(self, submit_sender: mpsc::Sender<OMSCmd>) {
        tracing::info!("MatchResultConsumer started");
        // todo: graceful shutdown
        // let mut stream = consumer.stream();
        let mut handlers = Vec::with_capacity(self.consumers.len());
        for (pair_str, consumer) in self.consumers.into_iter() {
            let sender = submit_sender.clone();
            let thread_id = format!("MatchResultConsumer-{}", pair_str);
            let h = tokio::spawn(async move {
                Self::run_pair_consumer(thread_id, consumer, sender).await;
            });
            handlers.push(h);
        }

        futures::future::join_all(handlers).await;
        info!("MatchResultConsumer stopped");
    }

    async fn run_pair_consumer(
        thread_id: String,
        consumer: rdkafka::consumer::StreamConsumer,
        sender: mpsc::Sender<OMSCmd>,
    ) {
        tracing::info!("{} up", thread_id);
        let mut stream = consumer.stream();
        while let Some(r) = stream.next().await {
            match r {
                Err(e) => {
                    error!("Kafka error: {:?}", e);
                    continue;
                }
                Ok(msg) => {
                    print_kafka_msg_meta(&msg);
                    match Self::parse_msg(msg) {
                        Ok(c) => {
                            if let Err(e) = Self::propose(&sender, c).await {
                                error!("{}: Failed to propose match result cmd {:?}", thread_id, e);
                                continue;
                            }
                        }
                        Err(e) => {
                            error!("{}: Failed to parse match result msg: {:?}", thread_id, e);
                            continue;
                        }
                    };
                }
            }
        }
    }

    fn parse_msg(msg: BorrowedMessage<'_>) -> Result<OMSCmd, OMSErr> {
        let payload = msg.payload().expect("valid payload");
        match BatchMatchResultTransfer::deserialize(payload) {
            Ok(batch_transfer) => Ok(OMSCmd::match_result_cmd(batch_transfer)),
            Err(e) => {
                error!("Failed to parse match result msg: {:?}", e);
                Err(OMSErr::new(
                    err_code::ERR_OMS_INVALID_MATCH_RESULT,
                    "invalid payload",
                ))
            }
        }
    }
}
