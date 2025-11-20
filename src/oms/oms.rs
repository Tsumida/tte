//!
//! OMS(Order Management System) module.
//!

#![allow(dead_code)]

use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;

use crate::common::err_code;
use crate::common::err_code::TradeEngineErr;
use crate::common::id::IDGenerator;
use crate::common::types::*;
use crate::ledger::spot;
use crate::ledger::spot::SpotLedger;
use crate::ledger::spot::SpotLedgerRPCHandler;
use crate::oms::error::OMSErr;
use crate::pbcode::oms;
use crate::pbcode::oms::BizAction;
use crate::pbcode::oms::TimeInForce;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone)]
struct AccountOrderList {
    bid_orders: BTreeMap<OrderID, OrderDetail>,
    ask_orders: BTreeMap<OrderID, OrderDetail>,
}

#[derive(Debug, Clone)]
struct SymbolMarketData {
    trade_pair: TradePair, // 交易对
    last_price: Decimal,   // 交易对最新成交价
    config: oms::TradePairConfig,
    last_match_id: MatchID, // oms接受match_result, 用于过滤已处理的match_id
}

#[derive(Debug, Clone)]
pub struct OMS {
    active_orders: BTreeMap<u64, AccountOrderList>, // account_id -> bid \ ask orders
    ledger: SpotLedger,
    client_order_map: HashMap<String, OrderID>, // client_order_id -> order_id
    last_seq_id: SeqID,                         // oms作为所有请求的sequencer
    market_data: HashMap<TradePair, SymbolMarketData>,
}

pub struct OMSChangeResult {
    pub spot_change_result: Option<spot::SpotChangeResult>,
}

impl OMS {
    pub fn new() -> Self {
        OMS {
            active_orders: BTreeMap::new(),
            ledger: SpotLedger::new(),
            client_order_map: HashMap::new(),
            last_seq_id: 0,
            market_data: HashMap::new(),
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
                trade_pair.clone(),
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

    pub fn process_trade_cmd(
        &mut self,
        cmd: Arc<oms::TradeCmd>,
    ) -> Result<OMSChangeResult, OMSErr> {
        match BizAction::from_i32(cmd.biz_action) {
            Some(oms::BizAction::PlaceOrder) => {
                let req = cmd.place_order_req.as_ref().ok_or_else(|| {
                    OMSErr::new(
                        err_code::ERR_INVALID_REQUEST,
                        "Missing field place_order_req",
                    )
                })?;
                let order = OrderBuilder::new().build(
                    req.order
                        .as_ref()
                        .ok_or_else(|| {
                            OMSErr::new(err_code::ERR_INVALID_REQUEST, "Missing field order")
                        })?
                        .clone(),
                )?;
                let total_fee = FeeCalculator {
                    volatile_limit: Decimal::from_str(
                        &self
                            .market_data
                            .get(&order.trade_pair)
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
                        .get(&order.trade_pair)
                        .ok_or_else(|| {
                            OMSErr::new(err_code::ERR_OMS_PAIR_NOT_FOUND, "Missing market data")
                        })?
                        .last_price,
                }
                .cal(&order);

                let spot_change_result = self
                    .ledger
                    .place_order(order, total_fee.frozen_amount)
                    .map_err(|e| OMSErr::new(e.code(), "SpotLedgerErr"))?; // todo: 优化错误传递
                Ok(OMSChangeResult {
                    spot_change_result: Some(spot_change_result),
                })
            }
            _ => {
                // ignore
                Ok(OMSChangeResult {
                    spot_change_result: None,
                })
            }
        }
    }

    pub fn seq_id(&self) -> SeqID {
        self.last_seq_id
    }

    pub fn check_place_order(&self, order: &Order) -> Result<(), OMSErr> {
        if let Some(config) = self.market_data.get(order.trade_pair()) {
            self.check_symbol_trading(config)?;
            self.check_price_in_range(*order.price(), config)?;
            self.check_qty(*order.target_qty(), config)?;
        } else {
            return Err(OMSErr::new(
                err_code::ERR_OMS_PAIR_NOT_FOUND,
                "Trading pair not found",
            ));
        }

        self.check_client_order_id(order.order_id())?;
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
                err_code::ERR_OMS_DUPLICATE_PLACE,
                "Duplicate client order ID",
            ));
        }

        Ok(())
    }
}

pub type OMSSnapshot = OMS;

pub(crate) struct OrderBuilder {
    create_order: bool,
}

impl OrderBuilder {
    pub fn new() -> Self {
        OrderBuilder { create_order: true }
    }

    pub fn build(&mut self, order: oms::Order) -> Result<Order, OMSErr> {
        Ok(Order {
            order_id: if self.create_order {
                IDGenerator::gen_order_id(order.account_id)
            } else {
                order.order_id
            },
            client_order_id: order.client_order_id.clone(),
            account_id: order.account_id,
            trade_pair: TradePair::from(order.trade_pair.unwrap()),
            direction: Direction::from_i32(order.direction).unwrap(),
            price: Decimal::from_str(&order.price)
                .map_err(|_| OMSErr::new(err_code::ERR_INVALID_REQUEST, "Invalid price"))?,
            target_qty: Decimal::from_str(&order.quantity)
                .map_err(|_| OMSErr::new(err_code::ERR_INVALID_REQUEST, "Invalid quantity"))?,
            post_only: order.post_only,
            order_type: OrderType::from_i32(order.order_type).unwrap(),
            seq_id: order.seq_id,
            prev_seq_id: order.prev_seq_id,
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
