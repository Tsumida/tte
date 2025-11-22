#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use crate::common::types::{MatchFlow, TradePair};
use crate::common::{err_code, types};
use crate::infra::kafka::{ConsumerConfig, ProducerConfig};
use crate::oms::error::OMSErr;
use crate::oms::oms::OMSRpcHandler;
use crate::sequencer::api::{DefaultSequencer, SequenceSetter};
use crate::{
    oms::oms::{OMS, OrderBuilder},
    pbcode::oms,
};
use futures::StreamExt;
use prost::Message as _;
use rdkafka::Message;
use rdkafka::message::BorrowedMessage;
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{error, info, instrument};

type InformSender = oneshot::Sender<Informer>;
type InformReceiver = oneshot::Receiver<Informer>;

#[derive(Debug, Clone)]
struct Informer {
    seq_id: u64,
    prev_seq_id: u64,
    is_success: bool,
    err: Option<OMSErr>,
}

type MatchReqSender = mpsc::Sender<Arc<Box<oms::TradeCmd>>>;
type MatchReqReceiver = mpsc::Receiver<Arc<Box<oms::TradeCmd>>>;

fn match_req_chan(chan_size: usize) -> (MatchReqSender, MatchReqReceiver) {
    mpsc::channel::<Arc<Box<oms::TradeCmd>>>(chan_size)
}

pub trait OrderRouter {
    fn route_key(&self) -> Option<&oms::TradePair>;
}

// refactor: 这个类型字段起来很麻烦;
#[derive(Debug)]
struct OMSCmd {
    cmd: Arc<Box<oms::TradeCmd>>,
    rsp_chan: Option<InformSender>,
}

impl SequenceSetter for OMSCmd {
    fn set_seq_id(&mut self, seq_id: u64, prev_seq_id: u64) {
        Arc::get_mut(&mut self.cmd).map(|cmd| {
            cmd.seq_id = seq_id;
            cmd.prev_seq_id = prev_seq_id;
        });
    }
}

impl OrderRouter for OMSCmd {
    fn route_key(&self) -> Option<&oms::TradePair> {
        self.cmd.route_key()
    }
}

impl OrderRouter for Arc<Box<oms::TradeCmd>> {
    fn route_key(&self) -> Option<&oms::TradePair> {
        match self.msg_types {
            1 => {
                // RpcCmd
                let rpc_cmd = self.rpc_cmd.as_ref().unwrap();
                rpc_cmd.trade_pair.as_ref()
            }
            2 => {
                // MatchResult
                self.match_result.as_ref().unwrap().trade_pair.as_ref()
            }
            _ => None,
        }
    }
}

impl OMSCmd {
    fn place_order_cmd(cmd: oms::PlaceOrderReq) -> (Self, InformReceiver) {
        let (rsp_chan, rsp_recv) = oneshot::channel();
        // refactor: avoid unwrap
        let trade_pair = cmd.order.as_ref().unwrap().trade_pair.as_ref().unwrap();
        (
            OMSCmd {
                cmd: Arc::new(Box::new(oms::TradeCmd {
                    seq_id: 0,
                    prev_seq_id: 0,
                    msg_types: 1,
                    rpc_cmd: Some(oms::RpcCmd {
                        trade_pair: Some(trade_pair.clone()),
                        biz_action: oms::BizAction::PlaceOrder as i32,
                        place_order_req: Some(cmd),
                        cancel_order_req: None,
                    }),
                    match_result: None,
                })),
                rsp_chan: Some(rsp_chan),
            },
            rsp_recv,
        )
    }

    fn cancel_order_cmd(cmd: oms::CancelOrderReq) -> (Self, InformReceiver) {
        let (rsp_chan, rsp_recv) = oneshot::channel();
        (
            OMSCmd {
                cmd: Arc::new(Box::new(oms::TradeCmd {
                    seq_id: 0,
                    prev_seq_id: 0,
                    msg_types: 1,
                    rpc_cmd: Some(oms::RpcCmd {
                        trade_pair: Some(oms::TradePair {
                            base: cmd.base.clone(),
                            quote: cmd.quote.clone(),
                        }),
                        biz_action: oms::BizAction::CancelOrder as i32,
                        place_order_req: None,
                        cancel_order_req: Some(cmd),
                    }),
                    match_result: None,
                })),
                rsp_chan: Some(rsp_chan),
            },
            rsp_recv,
        )
    }

    fn match_result_cmd(cmd: oms::TradeCmd) -> Self {
        OMSCmd {
            cmd: Arc::new(Box::new(cmd)),
            rsp_chan: None,
        }
    }
}

#[derive(Debug)]
pub struct TradeSystem {
    oms_view: Arc<RwLock<OMS>>, // read only
    submit_send: mpsc::Sender<OMSCmd>,
}

impl TradeSystem {
    pub async fn run_trade_system(
        oms: OMS,
        producer_cfgs: HashMap<String, ProducerConfig>,
        consumer_cfgs: HashMap<String, ConsumerConfig>,
    ) -> Result<(Self, Vec<tokio::task::JoinHandle<()>>), Box<dyn std::error::Error>> {
        let init_seq_id = oms.seq_id();
        let chan_size = 128;
        let (sequencer, submit_send, commit_recv) =
            DefaultSequencer::<OMSCmd>::new(init_seq_id, chan_size);
        let shared_oms = Arc::new(RwLock::new(oms));
        let mut apply_thread = ApplyThread {
            oms: shared_oms.clone(),
            commit_recv,
        };
        let (match_req_sender, match_req_receiver) = match_req_chan(chan_size);
        // todo: panic if dependencies fail
        // todo: graceful shutdown
        let mut match_req_thread = MatchRequestSender::new(
            match_req_receiver,
            HashMap::new(), // todo 提取
        )
        .init()
        .await
        .expect("init success");

        let mut handlers = vec![
            // sequencer thread
            tokio::spawn(async move {
                sequencer.run().await;
            }),
            // apply & response thread
            tokio::spawn(async move {
                apply_thread.run(Some(match_req_sender)).await;
            }),
            // match request sender
            tokio::spawn(async move { match_req_thread.run().await }),
        ];

        // todo: get all cfigs with match_req_<PAIR> & match_result_<PAIR>
        if let Some(cfg) = consumer_cfgs.get("match_result_BTCUSDT") {
            let mut match_result_consumer = MatchResultConsumer {
                submit_send: submit_send.clone(),
                cfg: cfg.clone(),
            };
            let consumer = match_result_consumer.cfg.subscribe()?;
            handlers.push(tokio::spawn(async move {
                let _ = match_result_consumer.run(consumer).await;
            }));
        }

        Ok((
            Self {
                oms_view: shared_oms,
                submit_send,
            },
            handlers,
        ))
    }

    async fn propose(&self, cmd: OMSCmd) -> Result<(), OMSErr> {
        self.submit_send.send(cmd).await.map_err(|e| {
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

        let ord = place_order_req.order.as_ref().unwrap();
        let order = OrderBuilder::new()
            .build(ord.clone())
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
            self.submit_send.send(cmd).await.map_err(|e| {
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
        req: tonic::Request<oms::TransferFreezeReq>,
    ) -> Result<tonic::Response<oms::TransferFreezeRsp>, tonic::Status> {
        todo!()
    }

    #[instrument(level = "info", skip_all)]
    async fn transfer(
        &self,
        req: tonic::Request<oms::TransferReq>,
    ) -> Result<tonic::Response<oms::TransferRsp>, tonic::Status> {
        todo!()
    }

    #[instrument(level = "info", skip_all)]
    async fn take_snapshot(
        &self,
        req: tonic::Request<oms::TakeSnapshotReq>,
    ) -> Result<tonic::Response<oms::TakeSnapshotRsp>, tonic::Status> {
        todo!()
    }

    #[instrument(level = "info", skip_all)]
    async fn update_trade_pair_config(
        &self,
        req: tonic::Request<oms::UpdateTradePairConfigReq>,
    ) -> Result<tonic::Response<oms::UpdateTradePairConfigRsp>, tonic::Status> {
        todo!()
    }
}

struct ReplayThread {
    reply_chan: oneshot::Sender<Informer>,
}

struct ApplyThread {
    oms: Arc<RwLock<OMS>>,
    commit_recv: mpsc::Receiver<OMSCmd>,
}

impl ApplyThread {
    async fn run(&mut self, match_req_sender: Option<mpsc::Sender<Arc<Box<oms::TradeCmd>>>>) {
        let batch_apply_size = 16;
        let mut batch = Vec::with_capacity(batch_apply_size);
        let mut results = Vec::with_capacity(batch_apply_size);

        tracing::info!("ApplyThread started");
        // try get a batch or update right now
        while let Some(cmd) = self.commit_recv.recv().await {
            batch.push(cmd);
            while batch.len() < batch_apply_size {
                match self.commit_recv.try_recv() {
                    Ok(cmd) => batch.push(cmd),
                    Err(_) => break,
                }
            }

            tracing::info!("ApplyThread processing batch of size {}", batch.len());

            let mut oms = self.oms.write().await;
            let mut informer = Vec::with_capacity(batch.len());
            let mut match_req_buffer = Vec::with_capacity(batch.len());
            for cmd_ext in batch.drain(..) {
                let cmd = cmd_ext.cmd;
                // Process each cmd with the oms instance
                if let Some(rpc_cmd) = &cmd.rpc_cmd {
                    results.push(oms.handle_rpc_cmd(rpc_cmd));
                    informer.push((cmd.clone(), cmd_ext.rsp_chan));
                    match_req_buffer.push(cmd.clone());
                } else {
                    error!("Unexpected non-rpc cmd in ApplyThread: {:?}", cmd);
                }
            }
            drop(oms); // critical: avoid deadlock
            for (cmd, ch) in informer.into_iter() {
                if ch.is_none() {
                    continue;
                }
                if let Err(_) = ch.unwrap().send(Informer {
                    seq_id: cmd.seq_id,
                    prev_seq_id: cmd.prev_seq_id,
                    is_success: true,
                    err: None,
                }) {
                    tracing::error!("response channel closed for action {:?}", cmd);
                }
            }

            batch.clear();
            results.clear();
        }
        if let Some(s) = match_req_sender {
            drop(s);
        }
        tracing::info!("ApplyThread stopped");
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
                    err_code::ERR_OMS_INVALID_MATCH_FLOW,
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
                    trade_pair: pair.clone(),
                    producers: producer,
                },
            );
        }
        Ok(self)
    }

    // 只处理RpcCmd
    fn is_rpc_cmd(cmd: &oms::TradeCmd) -> bool {
        cmd.msg_types == 1 && cmd.rpc_cmd.is_some()
    }

    async fn run(&mut self) {
        tracing::info!("MatchRequesrSender start");
        loop {
            let r = self.receiver.recv().await;
            if r.is_none() {
                break;
            }
            let cmd = r.expect("error");
            if !MatchRequestSender::is_rpc_cmd(&cmd.as_ref()) {
                error!("expect rpc cmd: {:?}", cmd);
                continue;
            }
            if let Some(trade_pair) = cmd.route_key() {
                if let Err(e) = self.route(trade_pair, &cmd).await {
                    error!("Failed to route match request: {:?}", e);
                    continue;
                }
            } else {
                error!("Missing trade pair in match request: {:?}", cmd);
                continue;
            }
        }
        info!("MatchRequestSender stopped");
    }

    async fn route(&self, trade_pair: &TradePair, cmd: &oms::TradeCmd) -> Result<(), OMSErr> {
        let pair = trade_pair.pair();
        if let Some(router) = self.routers.get(&pair) {
            let buf = MatchReqestProcesser::serialize(oms::BatchMatchRequest {
                trade_pair: Some(trade_pair.clone()),
                cmds: vec![cmd.clone()],
            });

            router
                .producers
                .send(
                    rdkafka::producer::FutureRecord::to("match_requests") // todo: topic from config
                        .key(&pair)
                        .payload(&buf), // todo: serialize
                    std::time::Duration::from_millis(500),
                )
                .await
                .map_err(|(e, _)| {
                    tracing::error!("Failed to send match request: {:?}", e);
                    OMSErr::new(err_code::ERR_INTERNAL, "kafka send failed")
                })?;
        }
        Ok(())
    }
}

struct MatchResultConsumer {
    submit_send: mpsc::Sender<OMSCmd>,
    cfg: ConsumerConfig,
}

impl MatchResultConsumer {
    fn parse_msg(&self, msg: BorrowedMessage<'_>) -> Result<OMSCmd, OMSErr> {
        let payload = msg.payload().expect("valid payload");
        match MatchFlow::deserialize(payload) {
            Ok(cmd) => {
                // check msg fields
                if cmd.msg_types != 2 {
                    return Err(OMSErr::new(
                        err_code::ERR_OMS_INVALID_MATCH_FLOW,
                        "msg_types is not MatchResult",
                    ));
                }
                if cmd.match_result.is_none() {
                    return Err(OMSErr::new(
                        err_code::ERR_OMS_INVALID_MATCH_FLOW,
                        "match_result is missing",
                    ));
                }
                if msg.payload().is_none() {
                    return Err(OMSErr::new(
                        err_code::ERR_OMS_INVALID_MATCH_FLOW,
                        "empty payload",
                    ));
                }

                Ok(OMSCmd::match_result_cmd(cmd))
            }
            Err(e) => {
                error!("Failed to deserialize MatchFlow: {:?}", e);
                Err(OMSErr::new(
                    err_code::ERR_OMS_INVALID_MATCH_FLOW,
                    "invalid payload",
                ))
            }
        }
    }

    async fn propose(&self, cmd: OMSCmd) -> Result<(), OMSErr> {
        self.submit_send.send(cmd).await.map_err(|e| {
            tracing::error!("Failed to propose match result cmd: {:?}", e);
            OMSErr::new(err_code::ERR_INTERNAL, "sequencer failed")
        })
    }

    async fn run(&mut self, consumer: rdkafka::consumer::StreamConsumer) -> Result<(), OMSErr> {
        tracing::info!("MatchResultConsumer started");
        // todo: graceful shutdown
        let mut stream = consumer.stream();
        while let Some(r) = stream.next().await {
            match r {
                Err(e) => {
                    error!("Kafka error: {:?}", e);
                    continue;
                }
                Ok(msg) => {
                    // process msg
                    tracing::info!(
                        "Received msg from (topic={}, partition={}, offset={}, ts={:?})",
                        msg.topic(),
                        msg.partition(),
                        msg.offset(),
                        msg.timestamp()
                    );

                    match self.parse_msg(msg) {
                        Ok(c) => {
                            self.propose(c).await?;
                        }
                        Err(e) => {
                            error!("Failed to parse match result msg: {:?}", e);
                            continue;
                        }
                    };
                }
            }
        }

        info!("MatchResultConsumer stopped");
        Ok(())
    }
}
