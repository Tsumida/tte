use std::collections::HashMap;

use rust_decimal::Decimal;

pub type Symbol = String; // BTCUSD
pub type OrderID = String;
pub type ClientOriginID = String;
pub type SeqID = u64;
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

// 订单结构体
#[derive(Debug, Clone)]
pub(crate) struct Order {
    pub(crate) order_id: OrderID,
    pub(crate) client_origin_id: ClientOriginID,
    pub(crate) seq_id: SeqID,
    pub(crate) prev_seq_id: SeqID,
    pub(crate) time_in_force: TimeInForce,
    pub(crate) order_type: OrderType,
    pub(crate) direction: Direction,
    pub(crate) price: Decimal,
    pub(crate) target_qty: Decimal, // taker目标成交数量
    pub(crate) post_only: bool,     // post only
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

// 撮合结果结构体
#[derive(Debug, Clone)]
pub(crate) struct MatchRecord {
    pub seq_id: SeqID,
    pub prev_seq_id: SeqID,
    pub price: Decimal,
    pub qty: Decimal,
    pub direction: Direction,
    pub taker_order_id: OrderID,
    pub maker_order_id: OrderID,
    pub is_taker_fulfilled: bool,
    pub is_maker_fulfilled: bool,
    pub symbol: Symbol,
}
