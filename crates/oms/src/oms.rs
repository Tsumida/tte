//!
//! OMS(Order Management System) module.
//!

#![allow(dead_code)]

use getset::Getters;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use tracing::error;

use crate::error::OMSErr;
use std::cmp::max;
use std::collections::HashSet;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use tte_core::err_code;
use tte_core::err_code::TradeEngineErr;
use tte_core::id::IDGenerator;
use tte_core::pbcode::oms;
use tte_core::pbcode::oms::BizAction;
use tte_core::pbcode::oms::TimeInForce;
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
    #[getset(get = "pub")]
    ledger: SpotLedger,
    // todo: 考虑基于定时器的缓存, 防止重复使用同一个client_order_id
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
    fn handle_success_fill(&mut self, record: &oms::FillRecord) -> Result<OMSChangeResult, OMSErr>;

    fn handle_success_cancel(
        &mut self,
        result: &oms::CancelResult,
    ) -> Result<OMSChangeResult, OMSErr>;
}

#[derive(Getters)]
pub struct OMSChangeResult {
    #[getset(get = "pub")]
    // 对账本的状态更新
    pub spot_change_result: Vec<Result<spot::SpotChangeResult, OMSErr>>,

    // pub order_event: Vec<OrderEvent>,
    #[getset(get = "pub")]
    // 需要转发给撮合器的请求
    // note：ReplaceOrder会依次生成Cancel\Place请求
    pub match_request: Option<oms::BatchMatchRequest>,
}

impl OMS {
    pub fn new() -> Self {
        OMS {
            active_orders: BTreeMap::new(),
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

    pub fn seq_id(&self) -> SeqID {
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

    pub fn get_order_detail(&self, account_id: u64, order_id: &str) -> Option<&OrderDetail> {
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

    pub fn get_order_detail_by_client_id(
        &self,
        account_id: u64,
        client_order_id: &str,
    ) -> Option<&OrderDetail> {
        match self.client_order_map.get(client_order_id) {
            Some(order_id) => self.get_order_detail(account_id, order_id),
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
    fn insert_order(&mut self, order: Order) -> Result<(), OMSErr> {
        let account_orders =
            self.active_orders
                .entry(order.account_id)
                .or_insert(AccountOrderList {
                    bid_orders: BTreeMap::new(),
                    ask_orders: BTreeMap::new(),
                });
        let order_id = order.order_id.clone();
        let client_order_id = order.client_order_id.clone();

        let order_detail = OrderDetail::new(order);
        let order_map = match order_detail.original().direction {
            Direction::Buy => &mut account_orders.bid_orders,
            Direction::Sell => &mut account_orders.ask_orders,
            _ => unreachable!(),
        };

        if let Some(exist_order) = order_map.insert(order_id.clone(), order_detail) {
            error!(
                "Order ID conflict: account_id={}, order_id={}",
                exist_order.original().account_id,
                order_id
            );
            return Err(OMSErr::new(err_code::ERR_INTERNAL, "order id conflict"));
        }

        self.client_order_map.insert(client_order_id, order_id);

        Ok(())
    }

    fn remove_order(
        &mut self,
        direction: Direction,
        account_id: u64,
        order_id: &str,
    ) -> Result<(), OMSErr> {
        // self.check_client_order_id(client_order_id)?;
        if let Some(orders) = self.active_orders.get_mut(&account_id) {
            let order_map = match direction {
                Direction::Buy => &mut orders.bid_orders,
                Direction::Sell => &mut orders.ask_orders,
                _ => unreachable!(),
            };
            match order_map.remove(order_id) {
                Some(filled_order) => {
                    let client_order_id = filled_order.original().client_order_id();
                    self.client_order_map.remove(client_order_id);
                    return Ok(());
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

    fn try_remove_filled_order(
        &mut self,
        direction: Direction,
        record: &oms::FillRecord,
    ) -> Result<(), OMSErr> {
        if record.is_taker_fulfilled {
            self.remove_order(direction, record.taker_account_id, &record.taker_order_id)?;
        }
        if record.is_maker_fulfilled {
            self.remove_order(
                reverse_direction(&direction),
                record.maker_account_id,
                &record.maker_order_id,
            )?;
        }
        Ok(())
    }
}

impl OMSRpcHandler for OMS {
    fn handle_rpc_cmd(
        &mut self,
        current_seq_id: u64,
        trade_pair: &TradePair,
        cmd: oms::RpcCmd,
    ) -> Result<OMSChangeResult, OMSErr> {
        match BizAction::from_i32(cmd.biz_action) {
            Some(oms::BizAction::PlaceOrder) => {
                let req = cmd.place_order_req.ok_or_else(|| {
                    OMSErr::new(
                        err_code::ERR_INVALID_REQUEST,
                        "Missing field place_order_req",
                    )
                })?;

                // note: 一般来说，sequencer持久化时固定ID，避免后续更新引入bug。
                // 但trade_id和一个sequencer_id关联，因此持久化后再生成是ok的
                let prev_trade_id = *self.id_manager.trade_id();
                let trade_id = self.id_manager.update_trade_id(current_seq_id);

                let req_order = req.order.as_ref().ok_or_else(|| {
                    OMSErr::new(err_code::ERR_INVALID_REQUEST, "Missing field order")
                })?;
                let mut order = OrderBuilder::new().build(trade_id, prev_trade_id, req_order)?;
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

                // todo: 账本和OMS整体并非原子操作, 需要考虑内存回滚机制
                // 更新活跃订单状态
                order.order_id = IDGenerator::gen_order_id(order.account_id);
                self.insert_order(order.clone())?;
                // 更新账本状态
                let match_request = Some(oms::BatchMatchRequest {
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
                });
                let spot_change_result = self
                    .ledger
                    .place_order(order, total_fee.frozen_amount)
                    .map_err(|e| OMSErr::new(e.code(), "SpotLedgerErr"))?; // todo: 优化错误传递
                Ok(OMSChangeResult {
                    spot_change_result: vec![Ok(spot_change_result)],
                    match_request,
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
    fn handle_success_fill(&mut self, record: &oms::FillRecord) -> Result<OMSChangeResult, OMSErr> {
        let mr = FillRecord::from(record);
        let match_id = mr.match_id;

        // 更新order状态
        let _ = self.try_remove_filled_order(mr.direction, record);

        // 更新ledger状态
        let spot_events = self
            .ledger
            .fill_order(&mr)
            .map_err(|e| {
                error!("match_id={}, err={}", match_id, e);
                OMSErr::new(
                    err_code::ERR_OMS_MATCH_RESULT_FAILED,
                    "match_result processing failed",
                )
            })?
            .into_iter()
            .map(|e| Ok(e))
            .collect::<Vec<Result<spot::SpotChangeResult, OMSErr>>>();

        // todo!
        Ok(OMSChangeResult {
            spot_change_result: spot_events,
            match_request: None,
        })
    }

    fn handle_success_cancel(
        &mut self,
        result: &oms::CancelResult,
    ) -> Result<OMSChangeResult, OMSErr> {
        let record = CancelOrderResult::from(result); // todo: error handling
        if result.order_state == OrderState::Cancelled as i32 {
            // 更新订单状态
            self.remove_order(record.direction, record.account_id, &record.order_id)?;

            // 更新账本状态
            self.ledger.cancel_order(&record).map_err(|e| {
                error!(
                    "cancel_order failed: account_id={}, order_id={}, err={}",
                    record.account_id, record.order_id, e
                );
                OMSErr::new(
                    err_code::ERR_OMS_MATCH_RESULT_FAILED,
                    "cancel_result processing failed",
                )
            })?;
        }

        Ok(OMSChangeResult {
            spot_change_result: vec![],
            match_request: None,
        })
    }
}

pub(crate) struct OrderBuilder {
    create_order: bool,
}

impl OrderBuilder {
    pub fn new() -> Self {
        OrderBuilder { create_order: true }
    }

    // risk: 保证和pb的完全一致
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
            direction: Direction::from_i32(order.direction).unwrap(),
            price: Decimal::from_str(&order.price)
                .map_err(|_| OMSErr::new(err_code::ERR_INVALID_REQUEST, "Invalid price"))?,
            target_qty: Decimal::from_str(&order.quantity)
                .map_err(|_| OMSErr::new(err_code::ERR_INVALID_REQUEST, "Invalid quantity"))?,
            post_only: order.post_only,
            order_type: OrderType::from_i32(order.order_type).unwrap(),
            trade_id: trade_id,
            prev_trade_id: prev_trade_id,
            time_in_force: TimeInForce::from_i32(order.time_in_force).unwrap(),
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
