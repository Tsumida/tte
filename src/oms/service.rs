//!

use std::sync::Arc;

use crate::common::types;
use crate::oms::error::OMSErr;
use crate::sequencer::api::{DefaultSequencer, SequenceSetter};
use crate::{
    oms::oms::{OMS, OrderBuilder},
    pbcode::oms,
};
use tokio::sync::{RwLock, broadcast, mpsc, oneshot};
use tracing::instrument;

// type OMSView = OMS;
type CmdExt = CmdWrapper<Arc<oms::TradeCmd>>;

impl SequenceSetter for CmdExt {
    fn set_seq_id(&mut self, seq_id: u64, prev_seq_id: u64) {
        Arc::get_mut(&mut self.cmd).map(|cmd| {
            cmd.seq_id = seq_id;
            cmd.prev_seq_id = prev_seq_id;
        });
    }
}

#[derive(Debug)]
struct TradeSystem {
    oms_view: Arc<RwLock<OMS>>, // read only
    submit_send: mpsc::Sender<CmdExt>,
}

struct ApplyThread {
    oms: Arc<RwLock<OMS>>,
    commit_recv: mpsc::Receiver<CmdExt>,
}

impl ApplyThread {
    async fn run(&mut self) {
        let batch_apply_size = 16;
        let mut batch = Vec::with_capacity(batch_apply_size);
        let mut results = Vec::with_capacity(batch_apply_size);

        // try get a batch or update right now
        while let Some(cmd) = self.commit_recv.recv().await {
            batch.push(cmd);
            while batch.len() < batch_apply_size {
                match self.commit_recv.try_recv() {
                    Ok(cmd) => batch.push(cmd),
                    Err(_) => break,
                }
            }
            let mut oms = self.oms.write().await;
            let mut informer = Vec::with_capacity(batch.len());
            for cmd_ext in batch.drain(..) {
                let cmd = cmd_ext.cmd;
                // Process each cmd with the oms instance
                results.push(oms.process_trade_cmd(cmd.clone()));
                informer.push((cmd.clone(), cmd_ext.rsp_chan));
            }

            // response thread
            tokio::spawn(async move {
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
                        tracing::error!(
                            "response channel closed for action {:?} seq_id {}",
                            cmd.biz_action,
                            cmd.seq_id
                        );
                    }
                }
            });

            batch.clear();
            results.clear();
        }
    }
}

impl TradeSystem {
    pub async fn run_trade_system(oms: OMS) -> Result<Self, Box<dyn std::error::Error>> {
        let init_seq_id = oms.seq_id();
        let chan_size = 128;
        let (sequencer, submit_send, commit_recv) =
            DefaultSequencer::<CmdExt>::new(init_seq_id, chan_size);

        let shared_oms = Arc::new(RwLock::new(oms));
        let mut apply_thread = ApplyThread {
            oms: shared_oms.clone(),
            commit_recv,
        };

        // todo: graceful shutdown
        _ = vec![
            // sequencer thread
            tokio::spawn(async move {
                sequencer.run().await;
            }),
            // apply & response thread
            tokio::spawn(async move {
                apply_thread.run().await;
            }),
        ];

        Ok(Self {
            oms_view: shared_oms,
            submit_send,
        })
    }
}

// Impl RPC Handler
#[tonic::async_trait]
impl oms::oms_service_server::OmsService for TradeSystem {
    #[instrument]
    async fn place_order(
        &self,
        req: tonic::Request<oms::PlaceOrderReq>,
    ) -> Result<tonic::Response<oms::PlaceOrderRsp>, tonic::Status> {
        let place_order_req = req.get_ref();
        if place_order_req.order.is_none() {
            return Err(tonic::Status::invalid_argument("Order detail is missing"));
        }

        if place_order_req.order.as_ref().unwrap().order_id.is_empty() {
            return Err(tonic::Status::invalid_argument("Order ID is missing"));
        }

        let ord = place_order_req.order.as_ref().unwrap();
        let order = OrderBuilder::new()
            .build(ord.clone())
            .map_err(|e| tonic::Status::invalid_argument(format!("Invalid order detail: {}", e)))?;

        let oms = self.oms_view.read().await;
        _ = oms.check_place_order(&order).map_err(|e| {
            tonic::Status::failed_precondition(format!("Precondition failed: {}", e))
        })?;

        // todo, write order into sequencer channel and wait for response.
        // let cmd = TradeCmdExt::place_order_cmd(place_order_req.clone(), rsp_chan);

        // delay = 1*consensus_delay
        let (cmd, rsp_recv) = CmdWrapper::place_order_cmd(place_order_req.clone());
        let start_time = std::time::Instant::now();
        let _ =
            self.submit_send.send(cmd).await.map_err(|e| {
                tonic::Status::internal(format!("Sequencer append failed: {:?}", e))
            })?;

        let _ = rsp_recv
            .await
            .map_err(|e| tonic::Status::internal(format!("Failed to receive response: {:?}", e)))?;

        tracing::info!(
            "PlaceOrder done, dur={} ms",
            start_time.elapsed().as_millis(),
        );

        Ok(tonic::Response::new(oms::PlaceOrderRsp {}))
    }

    #[instrument]
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

        let (cmd, rsp_recv) = CmdWrapper::cancel_order_cmd(cancel_order_req.clone());
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
}

type InformSender = oneshot::Sender<Informer>;
type InformReceiver = oneshot::Receiver<Informer>;

#[derive(Debug)]
struct CmdWrapper<T: Send + Sync + 'static> {
    cmd: T,
    rsp_chan: Option<InformSender>,
}

impl CmdWrapper<Arc<oms::TradeCmd>> {
    fn place_order_cmd(cmd: oms::PlaceOrderReq) -> (Self, InformReceiver) {
        let (rsp_chan, rsp_recv) = oneshot::channel();
        (
            CmdWrapper {
                cmd: Arc::new(oms::TradeCmd {
                    seq_id: 0,
                    prev_seq_id: 0,
                    biz_action: oms::BizAction::PlaceOrder as i32,
                    place_order_req: Some(cmd),
                    cancel_order_req: None,
                    admin_cmd: None,
                }),
                rsp_chan: Some(rsp_chan),
            },
            rsp_recv,
        )
    }

    fn cancel_order_cmd(cmd: oms::CancelOrderReq) -> (Self, InformReceiver) {
        let (rsp_chan, rsp_recv) = oneshot::channel();
        (
            CmdWrapper {
                cmd: Arc::new(oms::TradeCmd {
                    seq_id: 0,
                    prev_seq_id: 0,
                    biz_action: oms::BizAction::CancelOrder as i32,
                    place_order_req: None,
                    cancel_order_req: Some(cmd),
                    admin_cmd: None,
                }),
                rsp_chan: Some(rsp_chan),
            },
            rsp_recv,
        )
    }
}

#[derive(Debug, Clone)]
struct Informer {
    seq_id: u64,
    prev_seq_id: u64,
    is_success: bool,
    err: Option<OMSErr>,
}
