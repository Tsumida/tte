#![allow(dead_code)]

use core::panic;
use std::cmp::{Ordering, max};
use std::collections::HashMap;
use std::hash::Hash;
use std::{cmp::min, collections::BTreeMap};

use getset::Getters;
use rust_decimal::Decimal;

use tracing::{error, warn};
use tte_core::err_code;
use tte_core::pbcode::oms::{self, BizAction};
use tte_core::types::{
    CancelOrderResult, Direction, FillOrderResult, FillRecord, MatchResult, Order, OrderID,
    OrderState, OrderType, TimeInForce, TradePair,
};

// 订单簿键
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct OrderBookKey {
    price: Decimal,
    trade_id: u64,
}

impl OrderBookKey {
    fn new(price: Decimal, seq_id: u64) -> Self {
        return Self {
            price,
            trade_id: seq_id,
        };
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct MakerOrder {
    pub(crate) order: Order, // 不可变
    pub(crate) qty_info: MatchState,
    pub(crate) order_state: OrderState,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct MatchState {
    remain_qty: Decimal, // maker剩余数量
    filled_qty: Decimal, // taker已成交数量
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyExt<T: PartialEq + Eq + PartialOrd + Ord> {
    Bid(T),
    Ask(T),
}

impl<T: PartialEq + Eq + PartialOrd + Ord> KeyExt<T> {
    fn new(direction: Direction, key: T) -> Self {
        match direction {
            Direction::Buy => KeyExt::Bid(key),
            Direction::Sell => KeyExt::Ask(key),
            _ => panic!("invalid direction"), // 不能交叉对比
        }
    }
}

impl PartialOrd for KeyExt<OrderBookKey> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (KeyExt::Bid(a), KeyExt::Bid(b)) => {
                // bid: (price desc, seq_id asc)
                if a.price > b.price {
                    Some(Ordering::Less)
                } else if a.price < b.price {
                    Some(Ordering::Greater)
                } else {
                    a.trade_id.partial_cmp(&b.trade_id)
                }
            }
            (KeyExt::Ask(a), KeyExt::Ask(b)) => {
                // ask: (price asc , seq_id asc)
                if a.price < b.price {
                    Some(Ordering::Less)
                } else if a.price > b.price {
                    Some(Ordering::Greater)
                } else {
                    a.trade_id.partial_cmp(&b.trade_id)
                }
            }
            _ => panic!("invalid comparation"), // 不能交叉对比
        }
    }
}

impl Ord for KeyExt<OrderBookKey> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

// bid: (price desc, seq_id asc)
// ask: (price asc , seq_id asc)
pub(crate) struct BTreeOrderQueue {
    direction: Direction,
    orders: BTreeMap<KeyExt<OrderBookKey>, MakerOrder>,
}

impl BTreeOrderQueue {
    fn new(direction: Direction) -> Self {
        Self {
            direction,
            orders: BTreeMap::new(),
        }
    }

    fn orders(&self) -> &BTreeMap<KeyExt<OrderBookKey>, MakerOrder> {
        &self.orders
    }

    fn orders_mut(&mut self) -> &mut BTreeMap<KeyExt<OrderBookKey>, MakerOrder> {
        &mut self.orders
    }

    fn add(&mut self, key: OrderBookKey, order: MakerOrder) {
        _ = self.orders.insert(KeyExt::new(self.direction, key), order);
    }

    fn pop_min(&mut self) -> Option<(OrderBookKey, MakerOrder)> {
        self.orders.pop_first().map(|(k, v)| match k {
            KeyExt::Bid(key) | KeyExt::Ask(key) => (key, v),
        })
    }
    fn peek(&self) -> Option<(OrderBookKey, MakerOrder)> {
        self.orders.first_key_value().map(|(k, v)| match k {
            KeyExt::Bid(key) | KeyExt::Ask(key) => (key.clone(), v.clone()),
        })
    }

    // find next k-v bigger than give key
    fn next(&self, key: &OrderBookKey) -> Option<(OrderBookKey, MakerOrder)> {
        let search_key = KeyExt::new(self.direction, key.clone());
        let mut iter = self.orders.range((
            std::ops::Bound::Excluded(search_key),
            std::ops::Bound::Unbounded,
        ));
        iter.next().map(|(k, v)| match k {
            KeyExt::Bid(key) | KeyExt::Ask(key) => (key.clone(), v.clone()),
        })
    }

    fn get_aggregated_qty(&self, depth: usize) -> Vec<(Decimal, Decimal)> {
        let mut level = 0;
        let mut last_price = Decimal::ZERO;
        let mut current_qty = Decimal::ZERO;
        let mut agg = Vec::with_capacity(depth);
        for (_, order) in self.orders.iter() {
            if level >= depth {
                break;
            }
            if order.order.price != last_price {
                if last_price != Decimal::ZERO {
                    agg.push((last_price, current_qty));
                    level += 1;
                    if level >= depth {
                        break;
                    }
                }
                last_price = order.order.price;
                current_qty = order.qty_info.remain_qty;
            } else {
                current_qty += order.qty_info.remain_qty;
            }
        }
        // 判断是否要append最后一个level
        if level < depth {
            agg.push((last_price, current_qty));
        }

        agg
    }

    fn take_queue_snapshot(&self) -> Vec<MakerOrder> {
        self.orders.iter().map(|(_, maker)| maker.clone()).collect()
    }
}

impl From<Vec<MakerOrder>> for BTreeOrderQueue {
    fn from(value: Vec<MakerOrder>) -> BTreeOrderQueue {
        // 从序列构建BtreeMap
        let direction = value.first().unwrap().order.direction;
        let mut btree = BTreeMap::new();
        for v in value {
            btree.insert(
                KeyExt::new(
                    direction,
                    OrderBookKey {
                        price: v.order.price,
                        trade_id: v.order.trade_id,
                    },
                ),
                v,
            );
        }
        BTreeOrderQueue {
            direction: direction,
            orders: btree,
        }
    }
}

pub struct OrderBookLevelSnapshot {
    pub depth: usize,
    pub bid_orders: Vec<(Decimal, Decimal)>,
    pub ask_orders: Vec<(Decimal, Decimal)>,
    // pub last_seq_id: u64,
}

#[derive(Debug, Clone, Getters, serde::Serialize, serde::Deserialize)]
pub struct OrderBookSnapshot {
    #[get = "pub"]
    trade_pair: TradePair,
    #[get = "pub"]
    bid_orders: Vec<MakerOrder>,
    #[get = "pub"]
    ask_orders: Vec<MakerOrder>,
    #[get = "pub"]
    id_manager: IDManager,
    #[get( = "pub")]
    order_id_mapper: HashMap<OrderID, OrderBookKey>,
    #[get( = "pub")]
    last_price: Decimal,
}

pub(crate) trait OrderBookSnapshotHandler {
    fn take_snapshot_with_depth(
        &self,
        depth: usize,
    ) -> Result<OrderBookLevelSnapshot, OrderBookErr>;

    fn take_snapshot(&self) -> Result<OrderBookSnapshot, OrderBookErr>;

    fn from_snapshot(s: OrderBookSnapshot) -> Result<OrderBook, OrderBookErr>;
}

pub(crate) trait OrderBookRequestHandler {
    fn place_order(&mut self, order: Order) -> Result<MatchResult, OrderBookErr>;
    fn cancel_order(
        &mut self,
        order_id: OrderID,
        direction: Direction,
    ) -> Result<MatchResult, OrderBookErr>;
}

// MatchEngine专用ID管理
#[derive(Debug, Clone, Getters, Default, serde::Serialize, serde::Deserialize)]
pub struct IDManager {
    #[getset(get = "pub")]
    match_id: u64,

    #[getset(get = "pub")]
    trade_id: u64,

    #[getset(get = "pub")]
    seq_id: u64,
}

impl IDManager {
    pub fn new(init_match_id: u64) -> Self {
        Self {
            match_id: init_match_id,
            trade_id: 0,
            seq_id: 0,
        }
    }

    // OB调用, 所以可以自增
    fn advance_match_id(&mut self) -> u64 {
        self.match_id += 1;
        self.match_id
    }

    // todo: 幂等过滤
    // Sequencer-> OB
    fn update_seq_id(&mut self, current_seq_id: u64) -> u64 {
        self.seq_id = max(self.seq_id, current_seq_id);
        self.seq_id
    }

    // todo: 幂等过滤
    // OMS-> OB
    fn update_trade_id(&mut self, current_trade_id: u64) -> u64 {
        self.trade_id = max(self.trade_id, current_trade_id);
        self.trade_id
    }
}

pub struct OrderBook {
    id_manager: IDManager,
    order_id_mapper: HashMap<OrderID, OrderBookKey>,
    bid_orders: BTreeOrderQueue,
    ask_orders: BTreeOrderQueue,
    trade_pair: TradePair,
    last_price: Decimal,
}

// OrderBook需要保证顺序接收OBRequest
impl OrderBook {
    pub fn new(trade_pair: TradePair) -> Self {
        Self {
            id_manager: IDManager::default(),
            order_id_mapper: HashMap::new(),
            bid_orders: BTreeOrderQueue::new(Direction::Buy),
            ask_orders: BTreeOrderQueue::new(Direction::Sell),
            trade_pair,
            last_price: Decimal::ZERO,
        }
    }

    // todo: 考虑放到内部
    pub fn update_seq_id(&mut self, current_seq_id: u64) -> u64 {
        self.id_manager.update_seq_id(current_seq_id)
    }

    pub fn process_trade_cmd(
        &mut self,
        trade_cmd: oms::TradeCmd,
    ) -> Result<MatchResult, OrderBookErr> {
        if let Some(cmd) = trade_cmd.rpc_cmd {
            self.id_manager.update_trade_id(trade_cmd.trade_id);
            match BizAction::from_i32(cmd.biz_action) {
                Some(BizAction::PlaceOrder) => {
                    if let Some(oms::PlaceOrderReq {
                        order: Some(place_order),
                    }) = cmd.place_order_req
                    {
                        let order = Order::from_pb(place_order)
                            .map_err(|_| OrderBookErr::new(err_code::ERR_INVALID_REQUEST))?;
                        return self.place_order(order);
                    } else {
                        return Err(OrderBookErr::new(err_code::ERR_INVALID_REQUEST));
                    }
                }
                Some(BizAction::CancelOrder) => {
                    if let Some(req) = cmd.cancel_order_req {
                        let order_id = req.order_id;
                        let direction = Direction::from_i32(req.direction)
                            .ok_or(OrderBookErr::new(err_code::ERR_INVALID_REQUEST))?;
                        return self.cancel_order(order_id, direction);
                    } else {
                        return Err(OrderBookErr::new(err_code::ERR_INVALID_REQUEST));
                    }
                }
                _ => {
                    return Err(OrderBookErr::new(err_code::ERR_INVALID_REQUEST));
                }
            }
        } else {
            return Err(OrderBookErr::new(err_code::ERR_INVALID_REQUEST));
        }
    }

    pub fn process_admin_cmd(
        &mut self,
        _cmd: oms::MatchAdminCmd,
    ) -> Result<MatchResult, OrderBookErr> {
        // todo
        todo!()
    }

    // seq_id由raft模块生成
    fn post_order(&mut self, order: Order) -> Result<MatchResult, OrderBookErr> {
        // 直接加入订单簿
        let maker_order = MakerOrder {
            order: order.clone(),
            qty_info: MatchState {
                remain_qty: order.target_qty,
                filled_qty: Decimal::ZERO,
            },
            order_state: OrderState::New,
        };
        let queue = if order.direction == Direction::Buy {
            &mut self.bid_orders
        } else {
            &mut self.ask_orders
        };
        queue.add(
            OrderBookKey {
                price: order.price,
                trade_id: order.trade_id,
            },
            maker_order,
        );
        self.order_id_mapper.insert(
            order.order_id.clone(),
            OrderBookKey {
                price: order.price,
                trade_id: order.trade_id,
            },
        );
        Ok(MatchResult {
            action: BizAction::FillOrder,
            fill_result: Some(FillOrderResult {
                trade_pair: order.trade_pair.clone(),
                original_order: order,
                results: vec![],
                order_state: OrderState::New,
                total_filled_qty: Decimal::ZERO,
            }),
            cancel_result: None,
        })
    }

    // Require:
    // 1. Bid => taker_price >= maker_price
    // 2. Ask => taker_price <= maker_price
    fn match_price(taker_price: Decimal, maker_price: Decimal, last_price: Decimal) -> Decimal {
        // 参考: 交易所交易制度和规则深度解读，第95页
        // 连续竞价下以最后成交价、taker、maker的价格的中位数作为成交价，可以降低成交价的跳变. 这个思想和BTC的出块时间类似
        if last_price != Decimal::ZERO {
            let mut prices = [taker_price, maker_price, last_price];
            prices.sort();
            prices[1]
        } else {
            maker_price
        }
    }

    // critical
    // 按指定数量去撮合订单
    fn match_basic_order_by_qty(
        &mut self,
        mut taker: MakerOrder,
    ) -> Result<MatchResult, OrderBookErr> {
        let mut total_filled_qty = Decimal::ZERO;
        let size_hint = 8;
        let mut results = Vec::with_capacity(size_hint);
        let mut match_keys = Vec::with_capacity(size_hint);
        let (current_q, adversary_q) = if taker.order.direction == Direction::Buy {
            (&mut self.bid_orders, &mut self.ask_orders)
        } else {
            (&mut self.ask_orders, &mut self.bid_orders)
        };
        let mut filled_order_ids = Vec::with_capacity(size_hint);

        if let Some((mut maker_key, mut maker)) = adversary_q.peek() {
            // keep fill until:
            // 1. 无订单可成交
            // 2. taker完全成交
            // 3. 限价单价格不满足
            while total_filled_qty < taker.order.target_qty {
                if taker.order.order_type == OrderType::Limit
                    && ((taker.order.direction == Direction::Buy
                        && maker.order.price > taker.order.price)
                        || (taker.order.direction == Direction::Sell
                            && maker.order.price < taker.order.price))
                {
                    break;
                }

                let filled_qty = min(
                    taker.order.target_qty - total_filled_qty,
                    maker.qty_info.remain_qty,
                );
                match_keys.push(maker_key);
                let prev_match_id = self.id_manager.match_id().clone();
                let match_id = self.id_manager.advance_match_id();
                let match_price =
                    Self::match_price(taker.order.price, maker.order.price, self.last_price);
                let fill = FillRecord {
                    match_id: match_id,
                    prev_match_id: prev_match_id,
                    price: match_price,
                    qty: filled_qty,
                    direction: taker.order.direction,
                    taker_order_id: taker.order.order_id.clone(),
                    maker_order_id: maker.order.order_id.clone(),
                    is_taker_fulfilled: total_filled_qty + filled_qty >= taker.order.target_qty, // consider over match
                    is_maker_fulfilled: filled_qty >= maker.qty_info.remain_qty,
                    maker_state: if filled_qty >= maker.qty_info.remain_qty {
                        OrderState::Filled
                    } else {
                        OrderState::PartiallyFilled
                    },
                    taker_account_id: taker.order.account_id,
                    taker_state: if total_filled_qty + filled_qty >= taker.order.target_qty {
                        OrderState::Filled
                    } else {
                        OrderState::PartiallyFilled
                    },
                    maker_account_id: maker.order.account_id,
                    trade_pair: taker.order.trade_pair.clone(),
                };

                if fill.is_maker_fulfilled {
                    filled_order_ids.push(maker.order.order_id.clone());
                }
                if fill.is_taker_fulfilled {
                    filled_order_ids.push(taker.order.order_id.clone());
                }

                results.push(fill);
                total_filled_qty += filled_qty;
                if let Some((k, v)) = adversary_q.next(&maker_key) {
                    maker_key = k;
                    maker = v;
                } else {
                    break;
                }
            }
        }

        if total_filled_qty > taker.order.target_qty {
            // invalid path
            return Err(OrderBookErr::new(err_code::ERR_INPOSSIBLE_STATE));
        }

        // 更新taker订单状态
        let mut put_taker_in_current_q = false;
        match taker.order.time_in_force {
            TimeInForce::Gtk => {
                // 剩余部分进入订单簿
                taker.qty_info.filled_qty = total_filled_qty;
                taker.qty_info.remain_qty = taker.order.target_qty - total_filled_qty;
                if taker.qty_info.filled_qty >= taker.order.target_qty {
                    taker.order_state = OrderState::Filled;
                } else if taker.qty_info.filled_qty > Decimal::ZERO {
                    taker.order_state = OrderState::PartiallyFilled;
                    put_taker_in_current_q = true;
                } else {
                    taker.order_state = OrderState::New;
                    put_taker_in_current_q = true;
                }
            }
            TimeInForce::Fok => {
                // 完全成交或部分成交，剩余部分取消
                taker.qty_info.filled_qty = total_filled_qty;
                taker.order_state = if total_filled_qty >= taker.order.target_qty {
                    OrderState::Filled
                } else if total_filled_qty > Decimal::ZERO {
                    OrderState::PartiallyFilled
                } else {
                    OrderState::Cancelled
                }
            }
            TimeInForce::Ioc => {
                // 完全成交或取消
                taker.qty_info.filled_qty = total_filled_qty;
                taker.order_state = if total_filled_qty >= taker.order.target_qty {
                    OrderState::Filled
                } else {
                    OrderState::Cancelled
                };
            }
            _ => {
                return Err(OrderBookErr::new(err_code::ERR_OB_ORDER_TYPE_TIF));
            }
        }

        // 更新当前订单队列
        if put_taker_in_current_q {
            let ob_key = OrderBookKey {
                price: taker.order.price,
                trade_id: taker.order.trade_id,
            };
            self.order_id_mapper
                .insert(taker.order.order_id.clone(), ob_key.clone());
            current_q.add(ob_key, taker.clone());
        }
        // 更新对手方队列
        let n = match_keys.len();
        if n > 0 {
            let mut maker = None;
            for _ in 0..n {
                maker = adversary_q.pop_min();
            }
            if let Some(record) = results.last() {
                if !record.is_maker_fulfilled {
                    let mut maker = maker.unwrap();
                    maker.1.qty_info.remain_qty -= record.qty;
                    maker.1.order_state = OrderState::PartiallyFilled;
                    adversary_q.add(maker.0, maker.1);
                }
            }
        }

        // 清空已成交订单
        for order_id in filled_order_ids {
            self.order_id_mapper.remove(&order_id);
        }

        Ok(MatchResult {
            action: BizAction::FillOrder,
            fill_result: Some(FillOrderResult {
                trade_pair: taker.order.trade_pair.clone(),
                original_order: taker.order,
                results,
                order_state: taker.order_state,
                total_filled_qty,
            }),
            cancel_result: None,
        })
    }
}

impl OrderBookRequestHandler for OrderBook {
    fn place_order(&mut self, order: Order) -> Result<MatchResult, OrderBookErr> {
        self.id_manager.update_trade_id(order.trade_id);

        if order.post_only {
            return self.post_order(order);
        }

        // 前置处理
        // todo: 结合最小步进，超过的部分进行舍弃.
        // case:
        //      0.500000, min_step=0.000001,
        //      user: buy x / rate = 0.500000123456, quantize to 0.500000
        //
        // order.target_qty = quantize_qty(order.target_qty, config.min_step);

        match (order.order_type, order.time_in_force) {
            (OrderType::Limit, TimeInForce::Gtk)
            | (OrderType::Market, TimeInForce::Ioc)
            | (OrderType::Market, TimeInForce::Fok) => self.match_basic_order_by_qty(MakerOrder {
                order: order,
                qty_info: MatchState {
                    remain_qty: Decimal::ZERO, // 作为taker，为0
                    filled_qty: Decimal::ZERO,
                },
                order_state: OrderState::New,
            }),
            _ => Err(OrderBookErr::new(err_code::ERR_OB_ORDER_TYPE_TIF)), // 未实现其他类型
        }
    }

    fn cancel_order(
        &mut self,
        order_id: OrderID,
        direction: Direction,
    ) -> Result<MatchResult, OrderBookErr> {
        // 先检查当前订单是否存在
        // 再检查当前订单是否可以撤销
        match self.order_id_mapper.get(&order_id) {
            Some(ob_key) => {
                // 检查订单状态
                let queue = if direction == Direction::Buy {
                    &mut self.bid_orders
                } else {
                    &mut self.ask_orders
                };

                let order = queue
                    .orders()
                    .get(&KeyExt::new(direction, ob_key.clone()))
                    .ok_or_else(
                        ||{
                            warn!("inconsistent state: order_id_mapper contains order_id={}, but order not found in orderbook", order_id);
                            OrderBookErr::new(err_code::ERR_OB_ORDER_NOT_FOUND)
                        }
                    )?;

                match order.order_state {
                    OrderState::New | OrderState::PendingNew | OrderState::PartiallyFilled => {
                        // 可以撤销
                        if let Some(order) =
                            queue.orders.remove(&KeyExt::new(direction, ob_key.clone()))
                        {
                            // 可以撤销
                            self.order_id_mapper.remove(&order_id);
                            Ok(MatchResult {
                                action: BizAction::CancelOrder,
                                fill_result: None,
                                cancel_result: Some(CancelOrderResult {
                                    is_cancel_success: true,
                                    err_msg: None,
                                    trade_pair: order.order.trade_pair.clone(),
                                    direction: order.order.direction,
                                    order_id: order.order.order_id.clone(),
                                    account_id: order.order.account_id,
                                    order_state: OrderState::Cancelled,
                                }),
                            })
                        } else {
                            Err(OrderBookErr::new(err_code::ERR_OB_ORDER_NOT_FOUND))
                        }
                    }
                    OrderState::Filled => {
                        warn!("order already filled: {:?}", order.order_state);
                        return Err(OrderBookErr::new(err_code::ERR_OB_ORDER_FILLED));
                    }
                    OrderState::Cancelled => {
                        warn!("order already cancelled: {:?}", order.order_state);
                        return Err(OrderBookErr::new(err_code::ERR_OB_ORDER_CANCELED));
                    }
                    _ => {
                        warn!(
                            "invalid order state for cancellation: {:?}",
                            order.order_state
                        );
                        return Err(OrderBookErr::new(err_code::ERR_INVALID_REQUEST));
                    }
                }
            }
            None => {
                error!("order_id_mapper has no order_id={}", order_id);
                return Err(OrderBookErr::new(err_code::ERR_OB_ORDER_NOT_FOUND));
            }
        }
    }
}

impl OrderBookSnapshotHandler for OrderBook {
    fn take_snapshot_with_depth(
        &self,
        depth: usize,
    ) -> Result<OrderBookLevelSnapshot, OrderBookErr> {
        let bid_orders = self.bid_orders.get_aggregated_qty(depth);
        let ask_orders = self.ask_orders.get_aggregated_qty(depth);

        Ok(OrderBookLevelSnapshot {
            depth,
            bid_orders,
            ask_orders,
            // last_seq_id: self.seq_id,
        })
    }

    fn take_snapshot(&self) -> Result<OrderBookSnapshot, OrderBookErr> {
        Ok(OrderBookSnapshot {
            trade_pair: self.trade_pair.clone(),
            bid_orders: self.bid_orders.take_queue_snapshot(),
            ask_orders: self.ask_orders.take_queue_snapshot(),
            id_manager: self.id_manager.clone(),
            order_id_mapper: self.order_id_mapper.clone(),
            last_price: self.last_price,
        })
    }

    fn from_snapshot(s: OrderBookSnapshot) -> Result<OrderBook, OrderBookErr> {
        let n = s.ask_orders.len() + s.bid_orders.len();
        let mut order_id_to_key = HashMap::with_capacity(2 * n);
        for v in s.bid_orders.iter() {
            order_id_to_key.insert(
                v.order.order_id.clone(),
                OrderBookKey::new(v.order.price, v.order.trade_id),
            );
        }
        for v in s.ask_orders.iter() {
            order_id_to_key.insert(
                v.order.order_id.clone(),
                OrderBookKey::new(v.order.price, v.order.trade_id),
            );
        }

        let bid_orders = BTreeOrderQueue::from(s.bid_orders);
        let ask_orders = BTreeOrderQueue::from(s.ask_orders);

        Ok(OrderBook {
            trade_pair: s.trade_pair,
            id_manager: s.id_manager,
            order_id_mapper: order_id_to_key,
            bid_orders,
            ask_orders,
            last_price: Decimal::ZERO,
        })
    }
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct OrderBookErr {
    err_code: i32,
}

impl OrderBookErr {
    fn new(err_code: i32) -> Self {
        Self { err_code }
    }
}

impl std::fmt::Display for OrderBookErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OrderBookErr:{}", self.err_code)
    }
}

#[cfg(test)]
mod test {
    use crate::orderbook::{
        KeyExt, MatchResult, OrderBook, OrderBookErr, OrderBookKey, OrderBookRequestHandler,
        OrderBookSnapshotHandler,
    };
    use rust_decimal::Decimal;
    use rust_decimal::prelude::FromPrimitive;
    use rust_decimal_macros::dec;
    use tte_core::types::{Direction, Order, OrderState, OrderType, TimeInForce, TradePair};

    fn post_limit_order(
        ob: &mut OrderBook,
        account_id: u64,
        direction: Direction,
        price: f64,
        qty: f64,
    ) {
        let prev_trade_id = ob.id_manager.trade_id;
        let trade_id = prev_trade_id + 1;

        let client_order_id = format!("CLI_{}_{}", account_id, trade_id);
        let order_id = format!("ORD_{}_{}", account_id, trade_id);

        let order = Order {
            client_order_id,
            post_only: true, // essential
            account_id: account_id,
            order_id,
            trade_id: trade_id,
            prev_trade_id: prev_trade_id,
            direction: direction,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtk,
            price: Decimal::from_f64(price).unwrap(),
            target_qty: Decimal::from_f64(qty).unwrap(),
            trade_pair: TradePair {
                base: "BTC".to_string(),
                quote: "USD".to_string(),
            },
            stp_strategy: tte_core::pbcode::oms::StpStrategy::CancelTaker,
            create_time: chrono::Utc::now().timestamp_micros() as u64,
            version: 0,
        };
        _ = ob.place_order(order)
    }

    fn match_limit_order(
        ob: &mut OrderBook,
        account_id: u64,
        direction: Direction,
        price: f64,
        qty: f64,
    ) -> Result<MatchResult, OrderBookErr> {
        let prev_trade_id = ob.id_manager.trade_id;
        let trade_id = prev_trade_id + 1;
        let client_order_id = format!("CLI_{}_{}", account_id, trade_id);
        let order_id = format!("ORD_{}_{}", account_id, trade_id);
        let order = Order {
            client_order_id,
            post_only: false,
            order_id,
            account_id: account_id,
            trade_id: ob.id_manager.trade_id + 1,
            prev_trade_id: ob.id_manager.trade_id,
            direction: direction,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtk,
            price: Decimal::from_f64(price).unwrap(),
            target_qty: Decimal::from_f64(qty).unwrap(),
            trade_pair: TradePair {
                base: "BTC".to_string(),
                quote: "USD".to_string(),
            },
            stp_strategy: tte_core::pbcode::oms::StpStrategy::CancelTaker,
            create_time: chrono::Utc::now().timestamp_micros() as u64,
            version: 0,
        };
        ob.place_order(order)
    }

    #[test]
    fn test_key_ext() {
        let k1 = KeyExt::Bid(OrderBookKey {
            price: Decimal::from_f64(101.0).unwrap(),
            trade_id: 1,
        });
        let k2 = KeyExt::Bid(OrderBookKey {
            price: Decimal::from_f64(100.0).unwrap(),
            trade_id: 2,
        });
        assert!(k1 < k2);

        let k3 = KeyExt::Ask(OrderBookKey {
            price: Decimal::from_f64(100.0).unwrap(),
            trade_id: 1,
        });
        let k4 = KeyExt::Ask(OrderBookKey {
            price: Decimal::from_f64(101.0).unwrap(),
            trade_id: 2,
        });
        assert!(k3 < k4);
    }

    #[test]
    fn test_limit_no_fill() {
        // OMS:Bid,1001,CLI_1001_00101,LIMIT,BTC_USD,10050.00,0.5,GTK,1,false
        // OMS:Ask,1002,CLI_1002_00101,LIMIT,BTC_USD,10060.00,0.5,GTK,1,false
        // Bid-Ask <0, 不成交

        let mut ob = OrderBook::new(TradePair::new("BTC", "USD"));
        let r = match_limit_order(&mut ob, 1001, Direction::Buy, 10050.0, 0.5).unwrap();
        assert_eq!(r.fill_result.as_ref().unwrap().results.len(), 0);
        assert_eq!(r.fill_result.as_ref().unwrap().order_state, OrderState::New);

        let r2 = match_limit_order(&mut ob, 1002, Direction::Sell, 10060.0, 0.5).unwrap();
        assert_eq!(r2.fill_result.as_ref().unwrap().results.len(), 0);
        assert_eq!(
            r2.fill_result.as_ref().unwrap().order_state,
            OrderState::New
        );

        // 订单还在订单簿中
        assert!(ob.bid_orders.peek().is_some());
        assert!(ob.ask_orders.peek().is_some());
    }

    #[test]
    fn test_limit_buy_order_filled() {
        let mut ob = OrderBook::new(TradePair::new("BTC", "USD"));
        // ask_q: (100.0, 10.0), (101.0, 5.0), (102.0, 20.0)
        // bid: (102, 30.0)
        // match: (100.0,10.0), (101.0,5.0), (102.0,15.0)
        // ask_q_new: (102.0, 5.0)
        post_limit_order(&mut ob, 1001, Direction::Sell, 100.0, 10.0);
        post_limit_order(&mut ob, 1002, Direction::Sell, 101.0, 5.0);
        post_limit_order(&mut ob, 1003, Direction::Sell, 102.0, 20.0);
        let result = match_limit_order(&mut ob, 1004, Direction::Buy, 102.0, 30.0)
            .unwrap()
            .fill_result
            .unwrap();
        assert_eq!(result.results.len(), 3);
        assert_eq!(result.results[0].price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(result.results[0].qty, Decimal::from_f64(10.0).unwrap());
        assert_eq!(result.results[1].price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(result.results[1].qty, Decimal::from_f64(5.0).unwrap());
        assert_eq!(result.results[2].price, Decimal::from_f64(102.0).unwrap());
        assert_eq!(result.results[2].qty, Decimal::from_f64(15.0).unwrap());

        let (_, peek) = ob.ask_orders.peek().unwrap();
        assert_eq!(peek.order.price, Decimal::from_f64(102.0).unwrap());
        assert_eq!(peek.qty_info.remain_qty, Decimal::from_f64(5.0).unwrap());

        assert_eq!(result.order_state, OrderState::Filled);

        // 成交后order_id_mapper中无该订单
        assert!(
            ob.order_id_mapper
                .get(&result.original_order.order_id)
                .is_none()
        );
    }

    #[test]
    fn test_limit_buy_order_partially_filled() {
        let mut ob = OrderBook::new(TradePair::new("BTC", "USD"));
        // ask_q: (100.0, 10.0), (101.0, 5.0), (102.0, 20.0)
        // bid: (101, 30.0)
        // match: (100.0,10.0), (101.0,5.0)
        // ask_q_new: (102.0, 5.0)
        // bid_q_new: (102.0,15.0)
        post_limit_order(&mut ob, 1001, Direction::Sell, 100.0, 10.0);
        post_limit_order(&mut ob, 1002, Direction::Sell, 101.0, 5.0);
        post_limit_order(&mut ob, 1003, Direction::Sell, 102.0, 20.0);
        let result = match_limit_order(&mut ob, 1004, Direction::Buy, 101.0, 30.0)
            .unwrap()
            .fill_result
            .unwrap();
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.results[0].price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(result.results[0].qty, Decimal::from_f64(10.0).unwrap());
        assert_eq!(result.results[1].price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(result.results[1].qty, Decimal::from_f64(5.0).unwrap());

        let (_, ask_peek) = ob.ask_orders.peek().unwrap();
        assert_eq!(ask_peek.order.price, Decimal::from_f64(102.0).unwrap());
        assert_eq!(
            ask_peek.qty_info.remain_qty,
            Decimal::from_f64(20.0).unwrap()
        );

        let (_, bid_peek) = ob.bid_orders.peek().unwrap();
        assert_eq!(bid_peek.order.price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(
            bid_peek.qty_info.filled_qty,
            Decimal::from_f64(15.0).unwrap(),
        );
        assert_eq!(
            bid_peek.qty_info.remain_qty,
            Decimal::from_f64(15.0).unwrap(),
        );
        assert_eq!(bid_peek.order_state, OrderState::PartiallyFilled);

        // 未完全成交的订单仍在
        assert!(
            ob.order_id_mapper
                .get(&result.original_order.order_id)
                .is_some()
        );
        assert!(ob.order_id_mapper.get(&bid_peek.order.order_id).is_some());
    }

    #[test]
    fn test_limit_sell_order_filled() {
        let mut ob = OrderBook::new(TradePair::new("BTC", "USD"));
        // bid_q: (102.0, 10.0), (101.0, 5.0), (100.0, 20.0)
        // ask: (100, 30.0)
        // match:(102.0,10.0), (101.0,5.0), (100.0,15.0)
        // bid_q_new: (100.0, 5.0)
        post_limit_order(&mut ob, 1001, Direction::Buy, 102.0, 10.0);
        post_limit_order(&mut ob, 1002, Direction::Buy, 101.0, 5.0);
        post_limit_order(&mut ob, 1003, Direction::Buy, 100.0, 20.0);
        let result = match_limit_order(&mut ob, 1004, Direction::Sell, 100.0, 30.0)
            .unwrap()
            .fill_result
            .unwrap();
        assert_eq!(result.results.len(), 3);
        assert_eq!(result.results[0].price, Decimal::from_f64(102.0).unwrap());
        assert_eq!(result.results[0].qty, Decimal::from_f64(10.0).unwrap());
        assert_eq!(result.results[1].price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(result.results[1].qty, Decimal::from_f64(5.0).unwrap());
        assert_eq!(result.results[2].price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(result.results[2].qty, Decimal::from_f64(15.0).unwrap());

        let (_, peek) = ob.bid_orders.peek().unwrap();
        assert_eq!(peek.order.price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(peek.qty_info.remain_qty, Decimal::from_f64(5.0).unwrap());
    }

    #[test]
    fn test_limit_sell_order_partially_filled() {
        let mut ob = OrderBook::new(TradePair::new("BTC", "USD"));
        // bid_q: (102.0, 10.0), (101.0, 5.0), (100.0, 20.0)
        // ask_q: (101, 30.0)
        // match:(102.0,10.0), (101.0,5.0)
        // bid_q_new: (100.0,20.0)
        // ask_q_new: (101.0,15.0)

        post_limit_order(&mut ob, 1001, Direction::Buy, 102.0, 10.0);
        post_limit_order(&mut ob, 1002, Direction::Buy, 101.0, 5.0);
        post_limit_order(&mut ob, 1003, Direction::Buy, 100.0, 20.0);
        let result = match_limit_order(&mut ob, 1004, Direction::Sell, 101.0, 30.0)
            .unwrap()
            .fill_result
            .unwrap();
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.results[0].price, Decimal::from_f64(102.0).unwrap());
        assert_eq!(result.results[0].qty, Decimal::from_f64(10.0).unwrap());
        assert_eq!(result.results[1].price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(result.results[1].qty, Decimal::from_f64(5.0).unwrap());

        let (_, bid_peek) = ob.bid_orders.peek().unwrap();
        assert_eq!(bid_peek.order.price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(
            bid_peek.qty_info.remain_qty,
            Decimal::from_f64(20.0).unwrap()
        );
        // PS: 也就是一次都没有成交过; 只要有一次成交即是PartiallyFilled
        assert_eq!(bid_peek.order_state, OrderState::New);

        let (_, ask_peek) = ob.ask_orders.peek().unwrap();
        assert_eq!(ask_peek.order.price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(
            ask_peek.qty_info.remain_qty,
            Decimal::from_f64(15.0).unwrap()
        );
        assert_eq!(ask_peek.order_state, OrderState::PartiallyFilled);

        // bid_q: (100.0, 20.0)
        // ask_q: (101.0,15.0)
        // ask: (100, 2.0)
        // match:(100, 5.0)
        // bid_q_new: (100.0, 18.0)
        // ask_q_new: (101.0,15.0)
        let result2 = match_limit_order(&mut ob, 1005, Direction::Sell, 100.0, 2.0)
            .unwrap()
            .fill_result
            .unwrap();
        assert_eq!(result2.results.len(), 1);
        assert_eq!(result2.results[0].price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(result2.results[0].qty, Decimal::from_f64(2.0).unwrap());
        let (_, bid_peek2) = ob.bid_orders.peek().unwrap();
        assert_eq!(bid_peek2.order.price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(
            bid_peek2.qty_info.remain_qty,
            Decimal::from_f64(18.0).unwrap(),
        );
        assert_eq!(bid_peek2.order_state, OrderState::PartiallyFilled);

        let (_, ask_peek2) = ob.ask_orders.peek().unwrap();
        assert_eq!(ask_peek2.order.price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(
            ask_peek2.qty_info.remain_qty,
            Decimal::from_f64(15.0).unwrap(),
        );
    }

    #[test]
    fn test_snapshot() {
        let mut ob = OrderBook::new(TradePair::new("BTC", "USD"));
        // sufficient level
        post_limit_order(&mut ob, 1001, Direction::Buy, 102.0, 10.0);
        post_limit_order(&mut ob, 1002, Direction::Buy, 101.0, 5.0);
        post_limit_order(&mut ob, 1003, Direction::Buy, 101.0, 20.0);

        // insuffient level
        post_limit_order(&mut ob, 1011, Direction::Sell, 101.0, 20.0);
        post_limit_order(&mut ob, 1012, Direction::Sell, 101.0, 30.0);
        post_limit_order(&mut ob, 1013, Direction::Sell, 101.0, 25.0);

        // result:
        // (102, 10.0), (101, 25.0)
        // (101, 75.0)

        let snapshot = ob.take_snapshot_with_depth(2).unwrap();
        assert_eq!(
            vec![(dec!(102), dec!(10)), (dec!(101), dec!(25)),],
            snapshot.bid_orders,
        );
        assert_eq!(vec![(dec!(101), dec!(75)),], snapshot.ask_orders,);
        // assert_eq!(6, snapshot.last_seq_id);

        let snapshot = ob.take_snapshot_with_depth(1).unwrap();
        assert_eq!(vec![(dec!(102), dec!(10))], snapshot.bid_orders,);
        assert_eq!(vec![(dec!(101), dec!(75)),], snapshot.ask_orders,);
        // assert_eq!(6, snapshot.last_seq_id);
    }
}
