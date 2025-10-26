use rust_decimal::Decimal;

pub type OrderID = String;
pub type ClientOriginID = String;
pub type SeqID = u64;

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
    Pending,
    PartiallyFilled,
    Filled,
    Cancelled,
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

// 撮合结果结构体
#[derive(Debug, Clone)]
pub struct MatchRecord {
    pub(crate) seq_id: SeqID,
    pub(crate) prev_seq_id: SeqID,
    pub(crate) price: Decimal,
    pub(crate) qty: Decimal,
    pub(crate) direction: Direction,
    pub(crate) taker_order_id: OrderID,
    pub(crate) maker_order_id: OrderID,
    pub(crate) is_taker_fulfilled: bool,
    pub(crate) is_maker_fulfilled: bool,
}
