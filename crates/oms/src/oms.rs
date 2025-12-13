//!
//! OMS(Order Management System) module.
//!

#![allow(dead_code)]

use getset::Getters;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use tracing::{error, warn};

use crate::error::OMSErr;
use std::cmp::max;
use std::collections::HashSet;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use tte_core::err_code;
use tte_core::err_code::TradeEngineErr;
use tte_core::id::IDGenerator;
use tte_core::pbcode::oms::BizAction;
use tte_core::pbcode::oms::TimeInForce;
use tte_core::pbcode::oms::{self, OrderEvent};
use tte_core::types::*;
use tte_ledger::spot;
use tte_ledger::spot::SpotLedger;
use tte_ledger::spot::SpotLedgerMatchResultConsumer;
use tte_ledger::spot::SpotLedgerRPCHandler;

#[derive(Debug, Clone, Getters, serde::Serialize, serde::Deserialize)]
struct AccountOrderList {
    #[getset(get = "pub")]
    bid_orders: BTreeMap<OrderID, OrderDetail>,
    #[getset(get = "pub")]
    ask_orders: BTreeMap<OrderID, OrderDetail>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SymbolMarketData {
    trade_pair: TradePair, // 交易对
    last_price: Decimal,   // 交易对最新成交价
    config: oms::TradePairConfig,
    last_match_id: MatchID, // oms接受match_result, 用于过滤已处理的match_id
}

#[derive(Debug, Clone, Getters, serde::Serialize, serde::Deserialize)]
pub struct OMS {
    active_orders: BTreeMap<u64, AccountOrderList>, // account_id -> bid \ ask orders
    // todo: 历史订单持久化存储
    final_orders: BTreeMap<u64, Vec<OrderDetail>>, // account_id -> []orders
    #[getset(get = "pub")]
    ledger: SpotLedger,
    // todo: 考虑基于定时器的缓存, 防止重复使用同一个client_order_id导致数据关联错误
    client_order_map: HashMap<String, OrderID>, // client_order_id -> order_id
    market_data: HashMap<String, SymbolMarketData>, // trade_pair.pair() -> market data
    market_currencies: HashSet<String>,
    pub id_manager: IDManager,
}

#[derive(Getters, serde::Serialize, serde::Deserialize)]
pub struct OMSSnapshot {
    #[getset(get = "pub")]
    timestamp: u64,
    #[getset(get = "pub")]
    active_orders: BTreeMap<u64, AccountOrderList>, // account_id -> bid
    #[getset(get = "pub")]
    final_orders: BTreeMap<u64, Vec<OrderDetail>>, // account_id -> []orders
    #[getset(get = "pub")]
    ledger: SpotLedger,
    #[getset(get = "pub")]
    client_order_map: HashMap<String, OrderID>, // client_order_id -> order_id
    #[getset(get = "pub")]
    market_data: HashMap<String, SymbolMarketData>, // trade_pair.pair() ->
    #[getset(get = "pub")]
    market_currencies: HashSet<String>,
    #[getset(get = "pub")]
    id_manager: IDManager,
}

#[derive(Debug, Clone, Default, Getters, serde::Serialize, serde::Deserialize)]
pub struct IDManager {
    #[getset(get = "pub")]
    trade_id: u64,
    #[getset(get = "pub")]
    admin_id: u64,
    #[getset(get = "pub")]
    ledger_id: u64,
    #[getset(get = "pub")]
    seq_id: u64,
}

impl IDManager {
    pub fn update_seq_id(&mut self, seq_id: u64) -> u64 {
        self.seq_id = max(self.seq_id, seq_id);
        self.seq_id
    }

    pub fn update_trade_id(&mut self, seq_id: u64) -> u64 {
        self.trade_id = max(self.trade_id, seq_id);
        self.trade_id
    }

    pub fn update_admin_id(&mut self, seq_id: u64) -> u64 {
        self.admin_id = max(self.admin_id, seq_id);
        self.admin_id
    }

    pub fn update_ledger_id(&mut self, seq_id: u64) -> u64 {
        self.ledger_id = max(self.ledger_id, seq_id);
        self.ledger_id
    }
}

// 根据交易指令更新OMS的Ledger、活跃订单状态
pub trait OMSRpcHandler {
    fn handle_rpc_cmd(
        &mut self,
        current_seq_id: u64,
        trade_pair: &TradePair,
        cmd: oms::RpcCmd,
    ) -> Result<OMSChangeResult, OMSErr>;
}

// 根据撮合结果更新OMS的Ledger、活跃订单状态. 不处理失败的撮合结果
pub trait OMSMatchResultHandler {
    fn handle_success_fill(
        &mut self,
        trade_id: u64,
        prev_trade_id: u64,
        ts: u64,
        record: &oms::FillRecord,
    ) -> Result<OMSChangeResult, OMSErr>;

    fn handle_success_cancel(
        &mut self,
        trade_id: u64,
        prev_trade_id: u64,
        ts: u64,
        result: &oms::CancelResult,
    ) -> Result<OMSChangeResult, OMSErr>;
}

#[derive(Getters)]
pub struct OMSChangeResult {
    #[getset(get = "pub")]
    // 对账本的状态更新
    pub spot_change_result: Vec<spot::SpotChangeResult>,

    #[getset(get = "pub")]
    pub order_event: Vec<oms::OrderEvent>,

    #[getset(get = "pub")]
    // 需要转发给撮合器的请求
    // note：ReplaceOrder会依次生成Cancel\Place请求
    pub match_request: Option<oms::BatchMatchRequest>,
}

impl OMSChangeResult {
    pub fn empty() -> Self {
        OMSChangeResult {
            spot_change_result: vec![],
            order_event: vec![],
            match_request: None,
        }
    }
}

type BatchOMSTxResult = Result<Box<dyn FnMut(&mut OMS) -> Vec<oms::OrderEvent>>, OMSErr>;

impl OMS {
    pub fn new() -> Self {
        OMS {
            active_orders: BTreeMap::new(),
            final_orders: BTreeMap::new(),
            ledger: SpotLedger::new(),
            client_order_map: HashMap::new(),
            // last_seq_id: 0,
            market_data: HashMap::new(),
            market_currencies: HashSet::new(),
            id_manager: IDManager::default(),
        }
    }

    pub fn with_init_ledger(&mut self, balances: Vec<(u64, &str, f64)>) -> &mut Self {
        for (account_id, currency, amount) in balances {
            self.ledger
                .add_deposit(account_id, currency, Decimal::from_f64(amount).unwrap())
                .unwrap();
        }
        self
    }

    pub fn with_market_data(
        &mut self,
        market_data: Vec<(TradePair, Decimal, oms::TradePairConfig)>,
    ) -> &mut Self {
        for (trade_pair, last_price, config) in market_data {
            self.market_data.insert(
                trade_pair.pair(),
                SymbolMarketData {
                    trade_pair,
                    last_price,
                    config,
                    last_match_id: 0,
                },
            );
        }
        self
    }

    pub fn set_ids(req: &mut oms::Order, trade_id: u64, prev_trade_id: u64) {
        req.order_id = IDGenerator::gen_order_id(req.account_id);
        req.trade_id = trade_id;
        req.prev_trade_id = prev_trade_id;
    }

    pub fn seq_id(&self) -> u64 {
        self.id_manager.trade_id
    }

    pub fn check_place_order(&self, order: &Order) -> Result<(), OMSErr> {
        if let Some(config) = self.market_data.get(&order.trade_pair().pair()) {
            self.check_symbol_trading(config)?;
            self.check_price_in_range(*order.price(), config)?;
            self.check_qty(*order.target_qty(), config)?;
        } else {
            return Err(OMSErr::new(
                err_code::ERR_OMS_PAIR_NOT_FOUND,
                "Trading pair not found",
            ));
        }

        self.check_client_order_id(order.client_order_id())?;
        Ok(())
    }

    pub fn get_active_order_detail(&self, account_id: u64, order_id: &str) -> Option<&OrderDetail> {
        // check if order exists
        if let Some(account_orders) = self.active_orders.get(&account_id) {
            // bid and then ask
            if let Some(order) = account_orders.bid_orders.get(order_id) {
                return Some(order);
            }
            if let Some(order) = account_orders.ask_orders.get(order_id) {
                return Some(order);
            }
            None
        } else {
            None
        }
    }

    pub fn get_active_order_detail_by_client_id(
        &self,
        account_id: u64,
        client_order_id: &str,
    ) -> Option<&OrderDetail> {
        match self.client_order_map.get(client_order_id) {
            Some(order_id) => self.get_active_order_detail(account_id, order_id),
            None => None,
        }
    }

    pub fn get_ledger(&self) -> &SpotLedger {
        &self.ledger
    }

    pub fn take_snapshot(&self) -> OMSSnapshot {
        OMSSnapshot {
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            active_orders: self.active_orders.clone(),
            final_orders: self.final_orders.clone(),
            ledger: self.ledger.clone(),
            client_order_map: self.client_order_map.clone(),
            market_data: self.market_data.clone(),
            market_currencies: self.market_currencies.clone(),
            id_manager: self.id_manager.clone(),
        }
    }

    fn check_symbol_trading(&self, market_config: &SymbolMarketData) -> Result<(), OMSErr> {
        if market_config.config.state != oms::TradePairState::TradingPair as i32 {
            return Err(OMSErr::new(
                err_code::ERR_OMS_PAIR_NOT_TRADING,
                "Symbol not trading",
            ));
        }
        Ok(())
    }

    fn check_price_in_range(
        &self,
        price: Decimal,
        market_config: &SymbolMarketData,
    ) -> Result<(), OMSErr> {
        let config = &market_config.config;
        let (min_price, max_price) = (
            market_config.last_price
                * (Decimal::ONE - Decimal::from_str(&config.volatility_limit).unwrap()),
            market_config.last_price
                * (Decimal::ONE + Decimal::from_str(&config.volatility_limit).unwrap()),
        );
        if price < min_price || price > max_price {
            return Err(OMSErr::new(
                err_code::ERR_OMS_PRICE_OUT_OF_RANGE,
                "Price out of range",
            ));
        }
        Ok(())
    }

    fn check_qty(&self, qty: Decimal, market_config: &SymbolMarketData) -> Result<(), OMSErr> {
        if qty < Decimal::from_str(&market_config.config.min_quantity_increment).unwrap() {
            return Err(OMSErr::new(
                err_code::ERR_OMS_QTY_OUT_OF_RANGE,
                "Insufficient quantity",
            ));
        }
        Ok(())
    }

    fn check_client_order_id(&self, client_order_id: &str) -> Result<(), OMSErr> {
        if !client_order_id.is_empty() && self.client_order_map.contains_key(client_order_id) {
            return Err(OMSErr::new(
                err_code::ERR_OMS_DUPLICATE_CLI_ORD_ID,
                "Duplicate client order ID",
            ));
        }

        Ok(())
    }

    // refactor: atomic
    fn insert_order(
        &mut self,
        order: Order,
    ) -> Result<Box<dyn FnOnce(&mut OMS) -> Vec<OrderEvent>>, OMSErr> {
        // --- Compare/检查部分 ---
        let account_id = order.account_id;
        let order_id = order.order_id.clone();
        let client_order_id = order.client_order_id.clone();
        let direction = order.direction;

        if let Some(account_orders) = self.active_orders.get(&account_id) {
            let order_map = match direction {
                Direction::Buy => &account_orders.bid_orders,
                Direction::Sell => &account_orders.ask_orders,
                _ => unreachable!(),
            };

            if let Some(exist_order) = order_map.get(&order_id) {
                error!(
                    "Order ID conflict: account_id={}, order_id={}",
                    exist_order.original().account_id,
                    order_id
                );
                return Err(OMSErr::new(err_code::ERR_INTERNAL, "order id conflict"));
            }
        }
        let order_detail = OrderDetail::place_order(order.clone());
        let order_event = order_event_from_detail(&order_detail);

        Ok(Box::new(move |oms: &mut OMS| {
            let account_orders = oms
                .active_orders
                .entry(account_id) // 使用之前捕获的 account_id
                .or_insert(AccountOrderList {
                    bid_orders: BTreeMap::new(),
                    ask_orders: BTreeMap::new(),
                });

            let order_queue = match direction {
                Direction::Buy => &mut account_orders.bid_orders,
                Direction::Sell => &mut account_orders.ask_orders,
                // 检查阶段已经确保 direction 有效，unreachable! 可能是合理的
                _ => unreachable!(),
            };
            order_queue.insert(order_id.clone(), order_detail);
            oms.client_order_map.insert(client_order_id, order_id);
            vec![order_event]
        }))
    }

    // 订单不再活跃，移入终态订单列表
    fn inactivate_order(&mut self, account_id: u64, filled_order: OrderDetail) {
        let client_order_id = filled_order.original().client_order_id();
        self.client_order_map.remove(client_order_id);
        self.final_orders
            .entry(account_id)
            .or_insert(Vec::with_capacity(4))
            .push(filled_order);
    }

    // 更新订单状态、已成交数量。如果订单完全成交，从活跃订单删除。
    fn fill_order(
        &self,
        direction: Direction,
        account_id: u64,
        order_id: String,
        filled_qty: Decimal,
        state: OrderState,
        trade_id: u64,
        update_time: u64,
        is_full_fill: bool,
    ) -> Result<Box<dyn FnMut(&mut OMS) -> OrderDetail>, OMSErr> {
        let orders = self
            .active_orders
            .get(&account_id)
            .ok_or_else(|| OMSErr::new(err_code::ERR_OMS_ORDER_NOT_FOUND, "Account not found"))?;

        let order_map = match direction {
            Direction::Buy => &orders.bid_orders,
            Direction::Sell => &orders.ask_orders,
            _ => unreachable!(),
        };

        if order_map.get(&order_id).is_none() {
            return Err(OMSErr::new(
                err_code::ERR_OMS_ORDER_NOT_FOUND,
                "Order not found",
            ));
        }

        Ok(Box::new(move |oms: &mut OMS| {
            let orders = oms
                .active_orders
                .get_mut(&account_id)
                .expect("Account must exist as checked in fill_order pre-check");

            let om = match direction {
                Direction::Buy => &mut orders.bid_orders,
                Direction::Sell => &mut orders.ask_orders,
                _ => unreachable!(),
            };
            let mut order_detail = om.remove(&order_id).expect("must get");
            if is_full_fill {
                oms.inactivate_order(account_id, order_detail.clone());
            }
            order_detail.set_filled_qty(order_detail.filled_qty() + filled_qty);
            order_detail.set_last_trade_id(std::cmp::max(*order_detail.last_trade_id(), trade_id));
            order_detail.advance_version();
            order_detail.set_current_state(state);
            order_detail.set_update_time(update_time);
            order_detail
        }))
    }

    // Transaction
    // 更新订单成交量、状态等数据。如果完全成交，从活跃订单删除。
    fn fill_active_order(
        &mut self, // 保持 &mut self，用于调用 fill_order
        direction: Direction,
        trade_id: u64,
        record: &oms::FillRecord,
        ts: u64,
    ) -> BatchOMSTxResult {
        let qty = Decimal::from_str(&record.quantity).map_err(|_| {
            OMSErr::new(
                err_code::ERR_OMS_MATCH_RESULT_FAILED,
                "invalid fill quantity",
            )
        })?;
        let taker_state = OrderState::from_i32(record.taker_state).ok_or_else(|| {
            OMSErr::new(
                err_code::ERR_OMS_MATCH_RESULT_FAILED,
                "invalid taker order state",
            )
        })?;
        let maker_state = OrderState::from_i32(record.maker_state).ok_or_else(|| {
            OMSErr::new(
                err_code::ERR_OMS_MATCH_RESULT_FAILED,
                "invalid maker order state",
            )
        })?;
        let taker_account_id = record.taker_account_id;
        let maker_account_id = record.maker_account_id;
        let taker_order_id = record.taker_order_id.clone(); // 可能是 String，需要 Clone
        let maker_order_id = record.maker_order_id.clone(); // 可能是 String，需要 Clone
        let is_taker_fulfilled = record.is_taker_fulfilled;
        let is_maker_fulfilled = record.is_maker_fulfilled;
        let rev_direction = reverse_direction(&direction);

        // check阶段
        let mut taker_order_commit = self.fill_order(
            direction, // taker 方向
            taker_account_id,
            taker_order_id, // 传入 String
            qty,
            taker_state,
            trade_id,
            ts,
            is_taker_fulfilled,
        )?;

        let mut maker_order_commit = self.fill_order(
            rev_direction, // maker 方向相反
            maker_account_id,
            maker_order_id, // 传入 String
            qty,
            maker_state,
            trade_id,
            ts,
            is_maker_fulfilled,
        )?;

        // commit阶段
        Ok(Box::new(move |oms: &mut OMS| {
            let taker_detail = taker_order_commit(oms);
            let maker_detail = maker_order_commit(oms);
            let taker_event = order_event_from_detail(&taker_detail);
            let maker_event = order_event_from_detail(&maker_detail);
            vec![taker_event, maker_event]
        }))
    }

    fn cancel_active_order(
        &mut self,
        direction: Direction,
        account_id: u64,
        order_id: &str,
        ts: u64,
    ) -> Result<Vec<oms::OrderEvent>, OMSErr> {
        if let Some(orders) = self.active_orders.get_mut(&account_id) {
            let order_map = match direction {
                Direction::Buy => &mut orders.bid_orders,
                Direction::Sell => &mut orders.ask_orders,
                _ => unreachable!(),
            };
            match order_map.remove(order_id) {
                Some(mut canceled_order) => {
                    canceled_order.advance_version();
                    canceled_order.set_update_time(ts);
                    canceled_order.set_current_state(oms::OrderState::Cancelled);
                    let order_event = order_event_from_detail(&canceled_order);
                    self.inactivate_order(account_id, canceled_order);
                    return Ok(vec![order_event]);
                }
                None => {
                    return Err(OMSErr::new(
                        err_code::ERR_OMS_ORDER_NOT_FOUND,
                        "Order not found",
                    ));
                }
            }
        }
        Err(OMSErr::new(
            err_code::ERR_OMS_ORDER_NOT_FOUND,
            "Order not found",
        ))
    }
}

impl OMSRpcHandler for OMS {
    // Transaction
    fn handle_rpc_cmd(
        &mut self,
        current_seq_id: u64,
        trade_pair: &TradePair,
        cmd: oms::RpcCmd,
    ) -> Result<OMSChangeResult, OMSErr> {
        match BizAction::from_i32(cmd.biz_action) {
            Some(oms::BizAction::PlaceOrder) => {
                let mut req = cmd.place_order_req.ok_or_else(|| {
                    OMSErr::new(
                        err_code::ERR_INVALID_REQUEST,
                        "Missing field place_order_req",
                    )
                })?;

                // note: 一般来说，sequencer持久化时固定ID，避免后续更新引入bug。
                // 但trade_id和一个sequencer_id关联，因此持久化后再生成是ok的
                let prev_trade_id = *self.id_manager.trade_id();
                let trade_id = self.id_manager.update_trade_id(current_seq_id);
                let req_order = req.order.as_mut().ok_or_else(|| {
                    OMSErr::new(err_code::ERR_INVALID_REQUEST, "Missing field order")
                })?;
                Self::set_ids(req_order, trade_id, prev_trade_id);

                let order = OrderBuilder::new().build(trade_id, prev_trade_id, req_order)?;
                let pair = &order.trade_pair.pair();
                let total_fee = FeeCalculator {
                    volatile_limit: Decimal::from_str(
                        &self
                            .market_data
                            .get(pair)
                            .ok_or_else(|| {
                                OMSErr::new(err_code::ERR_OMS_PAIR_NOT_FOUND, "Missing market data")
                            })?
                            .config
                            .volatility_limit,
                    )
                    .map_err(|_| {
                        OMSErr::new(err_code::ERR_INVALID_REQUEST, "Invalid volatility limit")
                    })?,
                    last_price: self
                        .market_data
                        .get(pair)
                        .ok_or_else(|| {
                            OMSErr::new(err_code::ERR_OMS_PAIR_NOT_FOUND, "Missing market data")
                        })?
                        .last_price, // todo:
                }
                .cal(&order);

                // check阶段
                // 检查活跃订单状态
                // 检查账本状态
                let oms_commit = self.insert_order(order.clone())?;
                let ledger_commit = self
                    .ledger
                    .place_order(order, total_fee.frozen_amount)
                    .map_err(|e| OMSErr::new(e.code(), "SpotLedgerErr"))?;

                // commit阶段
                Ok(OMSChangeResult {
                    spot_change_result: vec![ledger_commit(&mut self.ledger)],
                    order_event: oms_commit(self),
                    match_request: Some(oms::BatchMatchRequest {
                        trade_pair: Some(trade_pair.clone()),
                        cmds: vec![oms::TradeCmd {
                            trade_id: trade_id.clone(),
                            prev_trade_id: prev_trade_id.clone(),
                            trade_pair: Some(trade_pair.clone()),
                            rpc_cmd: Some(oms::RpcCmd {
                                biz_action: BizAction::PlaceOrder as i32,
                                place_order_req: Some(req),
                                cancel_order_req: None,
                            }),
                        }],
                    }),
                })
            }
            Some(BizAction::CancelOrder) => {
                let prev_trade_id = self.id_manager.trade_id().clone();
                let trade_id = self.id_manager.update_trade_id(current_seq_id);
                // 无订单更新
                // 无账本更新
                // 转发撮合
                Ok(OMSChangeResult {
                    spot_change_result: vec![],
                    order_event: vec![],
                    match_request: Some(oms::BatchMatchRequest {
                        trade_pair: Some(trade_pair.clone()),
                        cmds: vec![oms::TradeCmd {
                            trade_id,
                            prev_trade_id,
                            trade_pair: Some(trade_pair.clone()),
                            rpc_cmd: Some(oms::RpcCmd {
                                biz_action: oms::BizAction::CancelOrder as i32,
                                place_order_req: None,
                                cancel_order_req: cmd.cancel_order_req,
                            }),
                        }],
                    }),
                })
            }
            _ => Err(OMSErr::new(
                err_code::ERR_INVALID_REQUEST,
                "Unsupported biz_action",
            )),
        }
    }
}

impl OMSMatchResultHandler for OMS {
    // Transaction
    fn handle_success_fill(
        &mut self,
        trade_id: u64,
        _prev_trade_id: u64,
        ts: u64,
        record: &oms::FillRecord,
    ) -> Result<OMSChangeResult, OMSErr> {
        let mr = FillRecord::from_pb(record).map_err(|e| {
            error!("FillRecord::from_pb failed: {}", e);
            OMSErr::new(err_code::ERR_OMS_MATCH_RESULT_FAILED, "invalid fill record")
        })?;
        let match_id = mr.match_id;

        // check阶段
        let mut oms_commit = self.fill_active_order(mr.direction, trade_id, record, ts)?;
        let ledger_commit = self.ledger.fill_order(&mr).map_err(|e| {
            error!("match_id={}, err={}", match_id, e);
            OMSErr::new(
                err_code::ERR_OMS_MATCH_RESULT_FAILED,
                "match_result processing failed",
            )
        })?;

        // commit阶段
        Ok(OMSChangeResult {
            spot_change_result: ledger_commit(&mut self.ledger),
            order_event: oms_commit(self),
            match_request: None,
        })
    }

    // Transaction
    fn handle_success_cancel(
        &mut self,
        _trade_id: u64,
        _prev_trade_id: u64,
        ts: u64,
        result: &oms::CancelResult,
    ) -> Result<OMSChangeResult, OMSErr> {
        let record = CancelOrderResult::from_pb(result).map_err(|e| {
            error!("CancelOrderResult::from failed: {}", e);
            OMSErr::new(
                err_code::ERR_OMS_MATCH_RESULT_FAILED,
                "invalid cancel result",
            )
        })?;
        if result.order_state == OrderState::Cancelled as i32 {
            // 更新订单状态
            let order_event = self.cancel_active_order(
                record.direction,
                record.account_id,
                &record.order_id,
                ts,
            )?;

            let ledger_commit = self.ledger.cancel_order(&record).map_err(|e| {
                error!(
                    "cancel_order failed: account_id={}, order_id={}, err={}",
                    record.account_id, record.order_id, e
                );
                OMSErr::new(
                    err_code::ERR_OMS_MATCH_RESULT_FAILED,
                    "cancel_result processing failed",
                )
            })?;

            // commit阶段
            return Ok(OMSChangeResult {
                spot_change_result: vec![ledger_commit(&mut self.ledger)],
                order_event,
                match_request: None,
            });
        }
        warn!(
            "CancelResult with non-cancelled state: account_id={}, order_id={}, state={}",
            record.account_id, record.order_id, result.order_state
        );
        Ok(OMSChangeResult::empty())
    }
}

pub(crate) struct OrderBuilder {
    create_order: bool,
}

impl OrderBuilder {
    pub fn new() -> Self {
        OrderBuilder { create_order: true }
    }

    pub fn build(
        &mut self,
        trade_id: u64,
        prev_trade_id: u64,
        order: &oms::Order,
    ) -> Result<Order, OMSErr> {
        Ok(Order {
            order_id: order.order_id.clone(),
            client_order_id: order.client_order_id.clone(),
            account_id: order.account_id,
            trade_pair: order.trade_pair.clone().ok_or_else(|| {
                OMSErr::new(err_code::ERR_INVALID_REQUEST, "Missing field trade_pair")
            })?,
            direction: Direction::from_i32(order.direction)
                .ok_or_else(|| OMSErr::new(err_code::ERR_INVALID_REQUEST, "invalid direction"))?,
            price: Decimal::from_str(&order.price)
                .map_err(|_| OMSErr::new(err_code::ERR_INVALID_REQUEST, "Invalid price"))?,
            target_qty: Decimal::from_str(&order.quantity)
                .map_err(|_| OMSErr::new(err_code::ERR_INVALID_REQUEST, "Invalid quantity"))?,
            post_only: order.post_only,
            order_type: OrderType::from_i32(order.order_type)
                .ok_or_else(|| OMSErr::new(err_code::ERR_INVALID_REQUEST, "invalid order type"))?,
            trade_id: trade_id,
            prev_trade_id: prev_trade_id,
            time_in_force: TimeInForce::from_i32(order.time_in_force).ok_or_else(|| {
                OMSErr::new(err_code::ERR_INVALID_REQUEST, "invalid time in force")
            })?,
            create_time: order.create_time,
            stp_strategy: oms::StpStrategy::from_i32(order.stp_strategy).ok_or_else(|| {
                OMSErr::new(err_code::ERR_INVALID_REQUEST, "invalid stp strategy")
            })?,
            version: order.version,
        })
    }
}

struct FeeCalculator {
    volatile_limit: Decimal,
    last_price: Decimal,
}

pub struct FeeTotalResult {
    pub frozen_amount: Decimal,
    pub fees: Vec<FeeItem>,
}

pub struct FeeItem {
    pub fee_config_id: u64,
    pub fee_account_id: u64, // 费率账户
    pub fee_amount: Decimal,
}

impl FeeCalculator {
    pub fn cal(&self, order: &Order) -> FeeTotalResult {
        let frozen_amount = self.calc_frozen_amount(order);
        // For simplicity, we assume a flat fee of 0.1% for all orders.
        let fee_rate = Decimal::new(1, 3); // 0.1%
        let fee_amount = frozen_amount * fee_rate;

        FeeTotalResult {
            frozen_amount,
            fees: vec![FeeItem {
                fee_config_id: 1,
                fee_account_id: order.account_id,
                fee_amount,
            }],
        }
    }

    fn calc_frozen_amount(&self, order: &Order) -> Decimal {
        match order.order_type {
            OrderType::Limit => self.calc_limit_frozen_amount(order),
            OrderType::Market => self.calc_market_frozen_amount(order),
            _ => Decimal::ZERO,
        }
    }

    fn calc_limit_frozen_amount(&self, order: &Order) -> Decimal {
        match order.direction {
            Direction::Buy => order.price * order.target_qty,
            Direction::Sell => order.target_qty,
            _ => Decimal::ZERO,
        }
    }

    fn calc_market_frozen_amount(&self, order: &Order) -> Decimal {
        match order.direction {
            Direction::Buy => {
                // qty * price * (1+v)
                order.target_qty * (self.last_price * (Decimal::ONE + self.volatile_limit))
            }
            Direction::Sell => order.target_qty,
            _ => Decimal::ZERO,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_atomic_place_order() {
        // Insufficent balance
        // place_order with BTC=100, but ledger has only BTC=50
        let balances = vec![
            (1001, "USDT", 1_000_000.0),
            (1001, "BTC", 50.0),
            (1001, "ETH", 500.0),
        ];
        let market_data = vec![
            (
                TradePair::new("BTC", "USDT"),
                dec!(80000.0),
                oms::TradePairConfig {
                    trade_pair: "BTCUSDT".to_string(),
                    min_price_increment: "0.01".to_string(),
                    min_quantity_increment: "0.0001".to_string(),
                    state: 1,
                    volatility_limit: "0.1".to_string(),
                },
            ),
            (
                TradePair::new("ETH", "USDT"),
                dec!(4000.0),
                oms::TradePairConfig {
                    trade_pair: "ETHUSDT".to_string(),
                    min_price_increment: "0.01".to_string(),
                    min_quantity_increment: "0.0001".to_string(),
                    state: 1,
                    volatility_limit: "0.1".to_string(),
                },
            ),
        ];
        let mut oms = OMS::new();
        oms.with_init_ledger(balances).with_market_data(market_data);

        let result = oms.handle_rpc_cmd(
            1,
            &TradePair::new("BTC", "USDT"),
            oms::RpcCmd {
                biz_action: BizAction::PlaceOrder as i32,
                place_order_req: Some(oms::PlaceOrderReq {
                    order: Some(oms::Order {
                        trade_id: 1,
                        prev_trade_id: 0,
                        order_id: "".to_string(),
                        client_order_id: "cli_ord_001".to_string(),
                        account_id: 1001,
                        trade_pair: Some(TradePair::new("BTC", "USDT")),
                        direction: Direction::Buy as i32,
                        price: "80000".to_string(),
                        quantity: "50.0".to_string(),
                        post_only: false,
                        order_type: OrderType::Limit as i32,
                        time_in_force: TimeInForce::Gtk as i32,
                        stp_strategy: oms::StpStrategy::CancelTaker as i32,
                        create_time: 0,
                        version: 0,
                    }),
                }),
                cancel_order_req: None,
            },
        );
        assert!(result.is_err());
        println!("Expected error: {:?}", result.err().unwrap());

        // expect: no balance changed
        let usdt_balance = oms.ledger.get_spot(1001, "USDT").unwrap();
        assert_eq!(usdt_balance.deposit(), &dec!(1_000_000.0));
        assert_eq!(usdt_balance.frozen(), &dec!(0.0));

        // expect: no active orders
        let account_orders = oms.active_orders.get(&1001);
        assert!(account_orders.is_none());
    }
}
