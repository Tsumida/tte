#![allow(dead_code)]

use crate::pbcode::oms::{self, BizAction};
use getset::Getters;
use prost::Message;
use rust_decimal::Decimal;

// #[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub type TradePair = oms::TradePair;

impl TradePair {
    pub fn new(base: &str, quote: &str) -> Self {
        TradePair {
            base: base.to_string(),
            quote: quote.to_string(),
        }
    }

    pub fn pair(&self) -> String {
        format!("{}{}", self.base, self.quote)
    }
}

// impl From<oms::TradePair> for TradePair {
//     fn from(tp: oms::TradePair) -> Self {
//         TradePair {
//             base: tp.base,
//             quote: tp.quote,
//         }
//     }
// }

// impl Into<oms::TradePair> for TradePair {
//     fn into(self) -> oms::TradePair {
//         oms::TradePair {
//             base: self.base,
//             quote: self.quote,
//         }
//     }
// }

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
#[derive(Debug, Clone, Getters)]
pub struct Order {
    #[getset(get = "pub", set = "pub")]
    pub order_id: OrderID,
    #[getset(get = "pub")]
    pub account_id: u64,
    #[getset(get = "pub")]
    pub client_order_id: ClientOrderID,
    #[getset(get = "pub")]
    pub seq_id: SeqID,
    #[getset(get = "pub")]
    pub prev_seq_id: SeqID,
    #[getset(get = "pub")]
    pub time_in_force: TimeInForce,
    #[getset(get = "pub")]
    pub order_type: OrderType,
    #[getset(get = "pub")]
    pub direction: Direction,
    #[getset(get = "pub")]
    pub price: Decimal,
    #[getset(get = "pub")]
    pub target_qty: Decimal, // taker目标成交数量
    #[getset(get = "pub")]
    pub post_only: bool, // post only
    #[getset(get = "pub")]
    pub trade_pair: TradePair,
}

impl Into<oms::Order> for Order {
    fn into(self) -> oms::Order {
        oms::Order {
            order_id: self.order_id,
            account_id: self.account_id,
            client_order_id: self.client_order_id,
            seq_id: self.seq_id,
            prev_seq_id: self.prev_seq_id,
            time_in_force: self.time_in_force as i32,
            order_type: self.order_type as i32,
            direction: self.direction as i32,
            price: self.price.to_string(),
            post_only: self.post_only,
            trade_pair: Some(self.trade_pair.into()),
            quantity: self.target_qty.to_string(),
            create_time: "".to_string(), // TODO
            stp_strategy: 0,             // TODO
        }
    }
}

impl Order {
    pub fn from_pb(om_order: oms::Order) -> Result<Self, Box<dyn std::error::Error>> {
        let price = Decimal::from_str_exact(&om_order.price)?;
        let quantity = Decimal::from_str_exact(&om_order.quantity)?;
        Ok(Self {
            order_id: om_order.order_id,
            account_id: om_order.account_id,
            client_order_id: om_order.client_order_id,
            seq_id: om_order.seq_id,
            prev_seq_id: om_order.prev_seq_id,
            time_in_force: oms::TimeInForce::from_i32(om_order.time_in_force)
                .unwrap_or(oms::TimeInForce::Gtk),
            order_type: oms::OrderType::from_i32(om_order.order_type)
                .unwrap_or(oms::OrderType::Limit),
            direction: oms::Direction::from_i32(om_order.direction).unwrap_or(oms::Direction::Buy),
            price,
            target_qty: quantity,
            post_only: om_order.post_only,
            trade_pair: om_order.trade_pair.unwrap(),
        })
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
    #[getset(get = "pub", set = "pub")]
    update_time: u64,
}

impl OrderDetail {
    pub fn new(order: &Order) -> Self {
        OrderDetail {
            original: order.clone(),
            current_state: OrderState::New,
            filled_qty: Decimal::new(0, 0),
            last_seq_id: 0,
            update_time: 0,
        }
    }
}

// from arena ?
impl Into<oms::OrderDetail> for &OrderDetail {
    fn into(self) -> oms::OrderDetail {
        oms::OrderDetail {
            original: Some(self.original.clone().into()),
            current_state: self.current_state.as_str_name().to_string(),
            filled_quantity: self.filled_qty.to_string(), // 考虑
            last_seq_id: self.last_seq_id,
            update_time: self.update_time,
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
    // pub replace_result: Option<ReplaceOrderResult>, // Some(_) if action == ReplaceOrder
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

pub struct TradeCmdTransfer {}

impl TradeCmdTransfer {
    pub fn serialize(msg: &oms::TradeCmd) -> Vec<u8> {
        msg.encode_to_vec()
    }

    pub fn deserialize(data: &[u8]) -> Result<oms::TradeCmd, prost::DecodeError> {
        oms::TradeCmd::decode(data)
    }
}

pub struct BatchMatchReqTransfer {}

impl BatchMatchReqTransfer {
    pub fn deserialize(data: &[u8]) -> Result<oms::BatchMatchRequest, prost::DecodeError> {
        oms::BatchMatchRequest::decode(data)
    }
}

pub struct BatchMatchResultTransfer {}

impl BatchMatchResultTransfer {
    pub fn serialize(msg: &oms::BatchMatchResult) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut buf = Vec::with_capacity(msg.encoded_len());
        msg.encode(&mut buf)?;
        Ok(buf)
    }
}
