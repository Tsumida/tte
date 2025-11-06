#![allow(dead_code)]

use crate::pbcode::oms::BizAction;
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct Symbol {
    pub base: String,
    pub quote: String,
}

impl ToString for Symbol {
    fn to_string(&self) -> String {
        format!("{}{}", self.base, self.quote)
    }
}

pub type OrderID = String;
pub type ClientOriginID = String;
pub type SeqID = u64;
pub type MatchID = u64;
pub type Currency = String; // USD, BTC, ETH

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Buy,
    Sell,
}

// 枚举定义
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    GTK,
    FOK,
    IOC,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderType {
    Limit,
    Market,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderState {
    New,             // The order is created
    Rejected,        // The order is rejected due to invaild parameter, balance insufficient, etc.
    PendingNew,      // The order is valid and waiting for further processing.
    PartiallyFilled, // The order is partially done.
    Filled,          // The order is completely done.
    Cancelled,       // The order is cancelled by user.
}

impl OrderState {
    pub fn is_final(&self) -> bool {
        match self {
            OrderState::Rejected | OrderState::Filled | OrderState::Cancelled => true,
            _ => false,
        }
    }
}

// 订单结构体
#[derive(Debug, Clone)]
pub(crate) struct Order {
    pub(crate) order_id: OrderID,
    pub(crate) account_id: u64,
    pub(crate) client_origin_id: ClientOriginID,
    pub(crate) seq_id: SeqID,
    pub(crate) prev_seq_id: SeqID,
    pub(crate) time_in_force: TimeInForce,
    pub(crate) order_type: OrderType,
    pub(crate) direction: Direction,
    pub(crate) price: Decimal,
    pub(crate) target_qty: Decimal, // taker目标成交数量
    pub(crate) post_only: bool,     // post only
    pub(crate) symbol: Symbol,
}

impl Order {
    fn order_id(&self) -> &OrderID {
        &self.order_id
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OrderDetail {
    original: Order,
    current_state: OrderState,
    filled_qty: Decimal,
    last_seq_id: SeqID,
}

impl OrderDetail {
    pub(crate) fn symbol(&self) -> &Symbol {
        &self.original.symbol
    }

    pub(crate) fn direction(&self) -> Direction {
        self.original.direction
    }

    pub(crate) fn price(&self) -> Decimal {
        self.original.price
    }

    pub(crate) fn quantity(&self) -> Decimal {
        self.original.target_qty
    }

    pub(crate) fn order_id(&self) -> &OrderID {
        &self.original.order_id
    }

    pub(crate) fn client_order_id(&self) -> &ClientOriginID {
        &self.original.client_origin_id
    }
}

// 撮合结果结构体
#[derive(Debug, Clone)]
pub(crate) struct MatchRecord {
    pub seq_id: SeqID,
    pub prev_seq_id: SeqID,
    pub match_id: MatchID,
    pub prev_match_id: MatchID,
    pub price: Decimal,
    pub qty: Decimal,
    pub direction: Direction,
    pub taker_order_id: OrderID,
    pub taker_account_id: u64,
    pub taker_state: OrderState,
    pub maker_order_id: OrderID,
    pub maker_account_id: u64,
    pub maker_state: OrderState,
    pub is_taker_fulfilled: bool,
    pub is_maker_fulfilled: bool,
    pub symbol: Symbol,
}

#[derive(Debug, Clone)]
pub(crate) struct MatchResult {
    pub action: BizAction,
    pub fill_result: Option<FillOrderResult>, // Some(_) if action == FillOrder
    pub replace_result: Option<ReplaceOrderResult>, // Some(_) if action == ReplaceOrder
    pub cancel_result: Option<CancelOrderResult>, // Some(_) if action == CancelOrder
}

#[derive(Debug, Clone)]
pub(crate) struct FillOrderResult {
    pub original_order: Order,
    pub symbol: Symbol,
    pub results: Vec<MatchRecord>,
    pub order_state: OrderState,
    pub total_filled_qty: Decimal,
}

#[derive(Debug, Clone)]
pub(crate) struct ReplaceOrderResult {
    pub is_replace_success: bool,
    pub err_msg: Option<String>,
    pub symbol: Symbol,
    pub direction: Direction,
    pub order_id: OrderID,
    pub account_id: u64,
    pub order_state: OrderState,
    pub new_price: Decimal,
    pub new_quantity: Decimal,
}

#[derive(Debug, Clone)]
pub(crate) struct CancelOrderResult {
    pub is_cancel_success: bool,
    pub err_msg: Option<String>,
    pub symbol: Symbol,
    pub direction: Direction,
    pub order_id: OrderID,
    pub account_id: u64,
    pub order_state: OrderState,
}

// generate test kit for placeOrder, cancelOrder
