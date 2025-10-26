use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::Hash;
use std::{cmp::min, collections::BTreeMap};

use rust_decimal::Decimal;

use crate::common::err_code;
use crate::common::types::{Direction, Order, OrderID, OrderState, OrderType, SeqID, TimeInForce};

#[derive(Debug, Clone)]
struct MatchRecord {
    seq_id: SeqID,
    prev_seq_id: SeqID,
    price: Decimal,
    qty: Decimal,
    direction: Direction,
    taker_order_id: OrderID,
    maker_order_id: OrderID,
    is_taker_fulfilled: bool,
    is_maker_fulfilled: bool,
}

#[derive(Debug, Clone)]
struct MatchResult {
    original_order: Order,
    results: Vec<MatchRecord>,
}

// 订单簿键
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct OrderBookKey {
    price: Decimal,
    seq_id: SeqID,
}

#[derive(Debug, Clone)]
struct MatchState {
    remain_qty: Decimal, // maker剩余数量
    filled_qty: Decimal, // taker已成交数量
    state: OrderState,
}

#[derive(Debug, Clone)]
struct MakerOrder {
    order: Order,            // 不可变
    match_state: MatchState, // 撮合过程中更新
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
        }
    }
}

impl PartialOrd for KeyExt<OrderBookKey> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (KeyExt::Bid(a), KeyExt::Bid(b)) => {
                // bid: (price desc, seq_id asc)
                if b.price > a.price {
                    Some(Ordering::Less)
                } else if b.price < a.price {
                    Some(Ordering::Greater)
                } else {
                    Some(a.seq_id.cmp(&b.seq_id))
                }
            }
            (KeyExt::Ask(a), KeyExt::Ask(b)) => {
                // ask: (price asc , seq_id asc)
                if a.price < b.price {
                    Some(Ordering::Less)
                } else if a.price > b.price {
                    Some(Ordering::Greater)
                } else {
                    Some(a.seq_id.cmp(&b.seq_id))
                }
            }
            _ => None, // 不能交叉对比
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

struct BTreeOrderQueue {
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
}

// // 卖单队列实现 (价格升序，时间升序)
struct OrderBook {
    seq_id: SeqID, // last seqID had ever seen. for dedup & ignore outdated request
    order_id_to_orderbook_key: HashMap<OrderID, OrderBookKey>,
    bid_orders: BTreeOrderQueue,
    ask_orders: BTreeOrderQueue,
}

impl OrderBook {
    fn new() -> Self {
        Self {
            seq_id: 0,
            order_id_to_orderbook_key: HashMap::new(),
            bid_orders: BTreeOrderQueue::new(Direction::Buy),
            ask_orders: BTreeOrderQueue::new(Direction::Sell),
        }
    }

    fn match_order(&mut self, order: Order) -> Result<MatchResult, OrderBookErr> {
        match (order.order_type, order.time_in_force) {
            (OrderType::Limit, TimeInForce::GTK)
            | (OrderType::Market, TimeInForce::IOC)
            | (OrderType::Market, TimeInForce::FOK) => self.match_basic_order(MakerOrder {
                order: order,
                match_state: MatchState {
                    remain_qty: Decimal::ZERO, // 作为taker，为0
                    filled_qty: Decimal::ZERO,
                    state: OrderState::Pending,
                },
            }),
            _ => Err(OrderBookErr::new(err_code::ERR_OB_ORDER_TYPE_TIF)), // 未实现其他类型
        }
    }

    fn match_basic_order(&mut self, mut taker: MakerOrder) -> Result<MatchResult, OrderBookErr> {
        let mut total_filled_qty = Decimal::ZERO;
        let size_hint = 8;
        let mut results = Vec::with_capacity(size_hint);
        let mut match_keys = Vec::with_capacity(size_hint);
        let (current_q, adversary_q) = if taker.order.direction == Direction::Buy {
            (&mut self.bid_orders, &mut self.ask_orders)
        } else {
            (&mut self.ask_orders, &mut self.bid_orders)
        };

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
                    maker.match_state.remain_qty,
                );
                match_keys.push(maker_key);
                results.push(MatchRecord {
                    seq_id: taker.order.seq_id,
                    prev_seq_id: taker.order.prev_seq_id,
                    price: maker.order.price,
                    qty: filled_qty,
                    direction: taker.order.direction,
                    taker_order_id: taker.order.order_id.clone(),
                    maker_order_id: maker.order.order_id.clone(),
                    is_taker_fulfilled: total_filled_qty + filled_qty >= taker.order.target_qty, // consider over match
                    is_maker_fulfilled: filled_qty >= maker.match_state.remain_qty,
                });

                total_filled_qty += filled_qty;
                if let Some((k, v)) = adversary_q.next(&maker_key) {
                    maker_key = k;
                    maker = v;
                } else {
                    break;
                }
            }
        }

        // 更新taker订单状态
        let mut put_taker_in_current_q = false;
        match taker.order.time_in_force {
            TimeInForce::GTK => {
                // 剩余部分进入订单簿
                taker.match_state.filled_qty = total_filled_qty;
                taker.match_state.remain_qty = taker.order.target_qty - total_filled_qty;
                if taker.match_state.filled_qty >= taker.order.target_qty {
                    taker.match_state.state = OrderState::Filled;
                } else if taker.match_state.filled_qty > Decimal::ZERO {
                    taker.match_state.state = OrderState::PartiallyFilled;
                    put_taker_in_current_q = true;
                } else {
                    taker.match_state.state = OrderState::Pending;
                    put_taker_in_current_q = true;
                }
            }
            TimeInForce::FOK => {
                // 完全成交或部分成交，剩余部分取消
                taker.match_state.filled_qty = total_filled_qty;
                taker.match_state.state = if total_filled_qty >= taker.order.target_qty {
                    OrderState::Filled
                } else if total_filled_qty > Decimal::ZERO {
                    OrderState::PartiallyFilled
                } else {
                    OrderState::Cancelled
                }
            }
            TimeInForce::IOC => {
                // 完全成交或取消
                taker.match_state.filled_qty = total_filled_qty;
                taker.match_state.state = if total_filled_qty >= taker.order.target_qty {
                    OrderState::Filled
                } else {
                    OrderState::Cancelled
                };
            }
        }

        // 更新当前订单队列
        if put_taker_in_current_q {
            current_q.add(
                OrderBookKey {
                    price: taker.order.price,
                    seq_id: taker.order.seq_id,
                },
                taker.clone(),
            );
        }
        // 更新对手方队列
        let n = match_keys.len();
        if n > 0 {
            let mut order = None;
            for _ in 0..n {
                order = adversary_q.pop_min();
            }
            if let Some(record) = results.last() {
                if !record.is_maker_fulfilled {
                    let mut order = order.unwrap();
                    order.1.match_state.remain_qty -= record.qty;
                    adversary_q.add(order.0, order.1);
                }
            }
        }

        Ok(MatchResult {
            original_order: taker.order,
            results: results,
        })
    }
}

#[derive(Debug)]
struct OrderBookErr {
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
