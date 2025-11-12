#![allow(dead_code)]

use crate::pbcode::oms::{self, BizAction};
use getset::Getters;
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TradePair {
    pub base: String,
    pub quote: String,
}

impl From<oms::TradePair> for TradePair {
    fn from(tp: oms::TradePair) -> Self {
        TradePair {
            base: tp.base,
            quote: tp.quote,
        }
    }
}

impl Into<oms::TradePair> for TradePair {
    fn into(self) -> oms::TradePair {
        oms::TradePair {
            base: self.base,
            quote: self.quote,
        }
    }
}

impl ToString for TradePair {
    fn to_string(&self) -> String {
        format!("{}{}", self.base, self.quote)
    }
}

pub type OrderID = String;
pub type ClientOrderID = String;
pub type SeqID = u64;
pub type MatchID = u64;
pub type Symbol = String; // USD, BTC, ETH
pub type Direction = oms::Direction;
pub type TimeInForce = oms::TimeInForce;
pub type OrderType = oms::OrderType;
pub type OrderState = oms::OrderState;

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
pub struct Order {
    pub order_id: OrderID,
    pub account_id: u64,
    pub client_order_id: ClientOrderID,
    pub seq_id: SeqID,
    pub prev_seq_id: SeqID,
    pub time_in_force: TimeInForce,
    pub order_type: OrderType,
    pub direction: Direction,
    pub price: Decimal,
    pub target_qty: Decimal, // taker目标成交数量
    pub post_only: bool,     // post only
    pub trade_pair: TradePair,
}

impl Order {
    pub fn order_id(&self) -> &OrderID {
        &self.order_id
    }

    pub fn set_seq_id(&mut self, seq_id: SeqID) {
        self.prev_seq_id = self.seq_id;
        self.seq_id = seq_id;
    }

    pub fn symbol(&self) -> &TradePair {
        &self.trade_pair
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }

    pub fn price(&self) -> Decimal {
        self.price
    }

    pub fn quantity(&self) -> Decimal {
        self.target_qty
    }

    pub fn client_order_id(&self) -> &ClientOrderID {
        &self.client_order_id
    }
}
#[derive(Debug, Clone, Getters)]
pub struct OrderDetail {
    #[getset(get = "pub")]
    original: Order,
    #[getset(get = "pub", set = "pub")]
    current_state: OrderState,
    #[getset(get = "pub", set = "pub")]
    filled_qty: Decimal,
    #[getset(get = "pub", set = "pub")]
    last_seq_id: SeqID,
}

impl OrderDetail {
    pub fn new(order: &Order) -> Self {
        OrderDetail {
            original: order.clone(),
            current_state: OrderState::New,
            filled_qty: Decimal::new(0, 0),
            last_seq_id: 0,
        }
    }
}
// 撮合结果结构体
#[derive(Debug, Clone)]
pub struct MatchRecord {
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
    pub trade_pair: TradePair,
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub action: BizAction,
    pub fill_result: Option<FillOrderResult>, // Some(_) if action == FillOrder
    pub replace_result: Option<ReplaceOrderResult>, // Some(_) if action == ReplaceOrder
    pub cancel_result: Option<CancelOrderResult>, // Some(_) if action == CancelOrder
}

impl MatchResult {
    pub fn get_fill_result(&self) -> Option<&FillOrderResult> {
        self.fill_result.as_ref()
    }
}

#[derive(Debug, Clone)]
pub struct FillOrderResult {
    pub original_order: Order,
    pub trade_pair: TradePair,
    pub results: Vec<MatchRecord>,
    pub order_state: OrderState,
    pub total_filled_qty: Decimal,
}

#[derive(Debug, Clone)]
pub struct ReplaceOrderResult {
    pub is_replace_success: bool,
    pub err_msg: Option<String>,
    pub trade_pair: TradePair,
    pub direction: Direction,
    pub order_id: OrderID,
    pub account_id: u64,
    pub order_state: OrderState,
    pub new_price: Decimal,
    pub new_quantity: Decimal,
}

#[derive(Debug, Clone)]
pub struct CancelOrderResult {
    pub is_cancel_success: bool,
    pub err_msg: Option<String>,
    pub trade_pair: TradePair,
    pub direction: Direction,
    pub order_id: OrderID,
    pub account_id: u64,
    pub order_state: OrderState,
}

// generate test kit for placeOrder, cancelOrder
