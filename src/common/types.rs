#![allow(dead_code)]

use crate::pbcode::oms as pb;
use getset::Getters;
use prost::Message;
use rust_decimal::Decimal;

// #[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub type TradePair = pb::TradePair;

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
pub type Direction = pb::Direction;
pub type TimeInForce = pb::TimeInForce;
pub type OrderType = pb::OrderType;
pub type OrderState = pb::OrderState;

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
    pub trade_id: SeqID,
    #[getset(get = "pub")]
    pub prev_trade_id: SeqID,
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

impl Into<pb::Order> for Order {
    fn into(self) -> pb::Order {
        pb::Order {
            order_id: self.order_id,
            account_id: self.account_id,
            client_order_id: self.client_order_id,
            trade_id: self.trade_id,
            prev_trade_id: self.prev_trade_id,
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
    pub fn from_pb(om_order: pb::Order) -> Result<Self, Box<dyn std::error::Error>> {
        let price = Decimal::from_str_exact(&om_order.price)?;
        let quantity = Decimal::from_str_exact(&om_order.quantity)?;
        Ok(Self {
            order_id: om_order.order_id,
            account_id: om_order.account_id,
            client_order_id: om_order.client_order_id,
            trade_id: om_order.trade_id,
            prev_trade_id: om_order.prev_trade_id,
            time_in_force: pb::TimeInForce::from_i32(om_order.time_in_force)
                .unwrap_or(pb::TimeInForce::Gtk),
            order_type: pb::OrderType::from_i32(om_order.order_type)
                .unwrap_or(pb::OrderType::Limit),
            direction: pb::Direction::from_i32(om_order.direction).unwrap_or(pb::Direction::Buy),
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
    last_trade_id: SeqID,
    #[getset(get = "pub", set = "pub")]
    update_time: u64,
}

impl OrderDetail {
    pub fn new(order: &Order) -> Self {
        OrderDetail {
            original: order.clone(),
            current_state: OrderState::New,
            filled_qty: Decimal::new(0, 0),
            last_trade_id: 0,
            update_time: 0,
        }
    }
}

// from arena ?
impl Into<pb::OrderDetail> for &OrderDetail {
    fn into(self) -> pb::OrderDetail {
        pb::OrderDetail {
            original: Some(self.original.clone().into()),
            current_state: self.current_state.as_str_name().to_string(),
            filled_quantity: self.filled_qty.to_string(), // 考虑
            last_trade_id: *self.last_trade_id(),
            update_time: self.update_time,
        }
    }
}

// 撮合结果结构体
#[derive(Debug, Clone)]
pub struct FillRecord {
    // pub seq_id: SeqID,
    // pub prev_seq_id: SeqID,
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

impl From<&pb::FillRecord> for FillRecord {
    fn from(mr: &pb::FillRecord) -> Self {
        FillRecord {
            match_id: mr.match_id,
            prev_match_id: mr.prev_match_id,
            price: Decimal::from_str_exact(&mr.price).unwrap_or(Decimal::new(0, 0)),
            qty: Decimal::from_str_exact(&mr.quantity).unwrap_or(Decimal::new(0, 0)),
            direction: pb::Direction::from_i32(mr.direction).unwrap_or(pb::Direction::Buy),
            taker_order_id: mr.taker_order_id.clone(),
            taker_account_id: mr.taker_account_id,
            taker_state: pb::OrderState::from_i32(mr.taker_state).unwrap_or(pb::OrderState::New),
            maker_order_id: mr.maker_order_id.clone(),
            maker_account_id: mr.maker_account_id,
            maker_state: pb::OrderState::from_i32(mr.maker_state).unwrap_or(pb::OrderState::New),
            is_taker_fulfilled: mr.is_taker_fulfilled,
            is_maker_fulfilled: mr.is_maker_fulfilled,
            trade_pair: mr.trade_pair.clone().unwrap(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub action: pb::BizAction,
    pub fill_result: Option<FillOrderResult>, // Some(_) if action == FillOrder
    // pub replace_result: Option<ReplaceOrderResult>, // Some(_) if action == ReplaceOrder
    pub cancel_result: Option<CancelOrderResult>, // Some(_) if action == CancelOrder
}

impl MatchResult {
    pub fn get_fill_result(&self) -> Option<&FillOrderResult> {
        self.fill_result.as_ref()
    }

    pub fn into_fill_result_pb(
        trade_id: u64,
        prev_trade_id: u64,
        fill_result: &FillOrderResult,
    ) -> pb::MatchResult {
        let records: Vec<pb::FillRecord> = fill_result
            .results
            .iter()
            .cloned()
            .map(|r| pb::FillRecord {
                match_id: r.match_id,
                prev_match_id: r.prev_match_id,
                price: r.price.to_string(),
                quantity: r.qty.to_string(),
                direction: r.direction as i32,
                taker_order_id: r.taker_order_id,
                taker_account_id: r.taker_account_id,
                taker_state: r.taker_state as i32,
                maker_order_id: r.maker_order_id,
                maker_account_id: r.maker_account_id,
                maker_state: r.maker_state as i32,
                is_taker_fulfilled: r.is_taker_fulfilled,
                is_maker_fulfilled: r.is_maker_fulfilled,
                trade_pair: Some(r.trade_pair.clone().into()),
            })
            .collect();
        pb::MatchResult {
            trade_id,
            prev_trade_id,
            action: pb::BizAction::FillOrder as i32,
            is_success: true,
            err_msg: "".to_string(),
            records,
            cancel_result: None,
        }
    }

    pub fn into_cancel_result_pb(
        trade_id: u64,
        prev_trade_id: u64,
        cancel_result: &CancelOrderResult,
    ) -> pb::MatchResult {
        pb::MatchResult {
            trade_id,
            prev_trade_id,
            action: pb::BizAction::CancelOrder as i32,
            is_success: true,
            err_msg: "".to_string(),
            records: vec![],
            cancel_result: Some(pb::CancelResult {
                trade_pair: Some(cancel_result.trade_pair.clone().into()),
                direction: cancel_result.direction as i32,
                order_id: cancel_result.order_id.clone(),
                account_id: cancel_result.account_id,
                order_state: cancel_result.order_state as i32,
            }),
        }
    }

    pub fn into_err(
        trade_id: u64,
        prev_trade_id: u64,
        action: pb::BizAction,
        err_msg: String,
    ) -> pb::MatchResult {
        pb::MatchResult {
            trade_id,
            prev_trade_id,
            action: action as i32,
            is_success: false,
            err_msg,
            records: vec![],
            cancel_result: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FillOrderResult {
    pub original_order: Order,
    pub trade_pair: TradePair,
    pub results: Vec<FillRecord>,
    pub order_state: OrderState,
    pub total_filled_qty: Decimal,
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

impl From<&pb::CancelResult> for CancelOrderResult {
    fn from(cr: &pb::CancelResult) -> Self {
        CancelOrderResult {
            is_cancel_success: cr.order_state == pb::OrderState::Cancelled as i32,
            err_msg: None,
            trade_pair: TradePair::new(
                &cr.trade_pair.as_ref().unwrap().base,
                &cr.trade_pair.as_ref().unwrap().quote,
            ),
            direction: pb::Direction::from_i32(cr.direction).unwrap_or(pb::Direction::Buy),
            order_id: cr.order_id.clone(),
            account_id: cr.account_id,
            order_state: pb::OrderState::from_i32(cr.order_state).unwrap_or(pb::OrderState::New),
        }
    }
}

pub struct TradeCmdTransfer {}

impl TradeCmdTransfer {
    pub fn serialize(msg: &pb::TradeCmd) -> Vec<u8> {
        msg.encode_to_vec()
    }

    pub fn deserialize(data: &[u8]) -> Result<pb::TradeCmd, prost::DecodeError> {
        pb::TradeCmd::decode(data)
    }
}

pub struct BatchMatchReqTransfer {}

impl BatchMatchReqTransfer {
    pub fn deserialize(data: &[u8]) -> Result<pb::BatchMatchRequest, prost::DecodeError> {
        pb::BatchMatchRequest::decode(data)
    }
}

pub struct BatchMatchResultTransfer {}

impl BatchMatchResultTransfer {
    pub fn serialize(msg: &pb::BatchMatchResult) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut buf = Vec::with_capacity(msg.encoded_len());
        msg.encode(&mut buf)?;
        Ok(buf)
    }

    pub fn deserialize(data: &[u8]) -> Result<pb::BatchMatchResult, prost::DecodeError> {
        pb::BatchMatchResult::decode(data)
    }
}
