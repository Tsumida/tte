use core::panic;
use std::cmp::{Ordering, max};
use std::collections::HashMap;
use std::hash::Hash;
use std::{cmp::min, collections::BTreeMap};

use rust_decimal::Decimal;
use rust_decimal::prelude::Zero;

use crate::common::err_code;
use crate::common::types::{Direction, Order, OrderID, OrderState, OrderType, SeqID, TimeInForce};

#[derive(Debug, Clone)]
pub(crate) struct MatchRecord {
    seq_id: SeqID,
    prev_seq_id: SeqID,
    price: Decimal,
    qty: Decimal,
    direction: Direction,
    taker_order_id: OrderID,
    maker_order_id: OrderID,
    is_taker_fulfilled: bool,
    is_maker_fulfilled: bool,
    maker_state: OrderState, // 对于maker，matchResult中只可能是F\PF; 在ob中，只能是Pending\PF
}

#[derive(Debug, Clone)]
pub(crate) struct MatchResult {
    original_order: Order,
    results: Vec<MatchRecord>,
    order_state: OrderState,
    total_filled_qty: Decimal,
}

// 订单簿键
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct OrderBookKey {
    price: Decimal,
    seq_id: SeqID,
}

impl OrderBookKey {
    fn new(price: Decimal, seq_id: SeqID) -> Self {
        return Self { price, seq_id };
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MakerOrder {
    pub(crate) order: Order, // 不可变
    pub(crate) match_state: MatchState,
    pub(crate) order_state: OrderState,
}

#[derive(Debug, Clone)]
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
                    a.seq_id.partial_cmp(&b.seq_id)
                }
            }
            (KeyExt::Ask(a), KeyExt::Ask(b)) => {
                // ask: (price asc , seq_id asc)
                if a.price < b.price {
                    Some(Ordering::Less)
                } else if a.price > b.price {
                    Some(Ordering::Greater)
                } else {
                    a.seq_id.partial_cmp(&b.seq_id)
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
        let mut cnt = 0;
        let mut last_price = Decimal::ZERO;
        let mut current_qty = Decimal::ZERO;
        let mut agg = Vec::with_capacity(depth);
        for (_, order) in self.orders.iter() {
            if cnt >= depth {
                break;
            }
            if last_price != order.order.price {
                last_price = order.order.price;
                current_qty += order.match_state.remain_qty;
                agg.push((last_price, current_qty));
                cnt += 1;

                current_qty = Decimal::ZERO;
            }
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
                        seq_id: v.order.seq_id,
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
    pub last_seq_id: u64,
}

pub struct OrderBookSnapshot {
    bid_orders: Vec<MakerOrder>,
    ask_orders: Vec<MakerOrder>,
    last_seq_id: u64,
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
}

pub(crate) struct OrderBook {
    seq_id: SeqID, // last seqID had ever seen. for dedup & ignore outdated request
    order_id_to_orderbook_key: HashMap<OrderID, OrderBookKey>,
    bid_orders: BTreeOrderQueue,
    ask_orders: BTreeOrderQueue,
}

// OrderBook需要保证顺序接收OBRequest
impl OrderBook {
    pub fn new() -> Self {
        Self {
            seq_id: 0,
            order_id_to_orderbook_key: HashMap::new(),
            bid_orders: BTreeOrderQueue::new(Direction::Buy),
            ask_orders: BTreeOrderQueue::new(Direction::Sell),
        }
    }

    fn advance_seq_id(&mut self, seq_id: u64) {
        self.seq_id = max(self.seq_id, seq_id);
    }

    fn post_order(&mut self, order: Order) -> Result<MatchResult, OrderBookErr> {
        // 直接加入订单簿
        let maker_order = MakerOrder {
            order: order.clone(),
            match_state: MatchState {
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
                seq_id: order.seq_id,
            },
            maker_order,
        );
        self.order_id_to_orderbook_key.insert(
            order.order_id.clone(),
            OrderBookKey {
                price: order.price,
                seq_id: order.seq_id,
            },
        );
        Ok(MatchResult {
            original_order: order,
            results: vec![],
            order_state: OrderState::New,
            total_filled_qty: Decimal::ZERO,
        })
    }

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
                    maker_state: if filled_qty >= maker.match_state.remain_qty {
                        OrderState::Filled
                    } else {
                        OrderState::PartiallyFilled
                    },
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

        if total_filled_qty > taker.order.target_qty {
            // invalid path
            return Err(OrderBookErr::new(err_code::ERR_OB_INTERNAL));
        }

        // 更新taker订单状态
        let mut put_taker_in_current_q = false;
        match taker.order.time_in_force {
            TimeInForce::GTK => {
                // 剩余部分进入订单簿
                taker.match_state.filled_qty = total_filled_qty;
                taker.match_state.remain_qty = taker.order.target_qty - total_filled_qty;
                if taker.match_state.filled_qty >= taker.order.target_qty {
                    taker.order_state = OrderState::Filled;
                } else if taker.match_state.filled_qty > Decimal::ZERO {
                    taker.order_state = OrderState::PartiallyFilled;
                    put_taker_in_current_q = true;
                } else {
                    taker.order_state = OrderState::New;
                    put_taker_in_current_q = true;
                }
            }
            TimeInForce::FOK => {
                // 完全成交或部分成交，剩余部分取消
                taker.match_state.filled_qty = total_filled_qty;
                taker.order_state = if total_filled_qty >= taker.order.target_qty {
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
                taker.order_state = if total_filled_qty >= taker.order.target_qty {
                    OrderState::Filled
                } else {
                    OrderState::Cancelled
                };
            }
        }

        // 更新当前订单队列
        if put_taker_in_current_q {
            self.order_id_to_orderbook_key.insert(
                taker.order.order_id.clone(),
                OrderBookKey {
                    price: taker.order.price,
                    seq_id: taker.order.seq_id,
                },
            );
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
            let mut maker = None;
            for _ in 0..n {
                maker = adversary_q.pop_min();
            }
            if let Some(record) = results.last() {
                if !record.is_maker_fulfilled {
                    let mut maker = maker.unwrap();
                    maker.1.match_state.remain_qty -= record.qty;
                    maker.1.order_state = OrderState::PartiallyFilled;
                    adversary_q.add(maker.0, maker.1);
                }
            }
        }

        Ok(MatchResult {
            original_order: taker.order,
            results: results,
            order_state: taker.order_state,
            total_filled_qty: total_filled_qty,
        })
    }
}

impl OrderBookRequestHandler for OrderBook {
    fn place_order(&mut self, order: Order) -> Result<MatchResult, OrderBookErr> {
        if order.seq_id <= self.seq_id {
            return Err(OrderBookErr::new(err_code::ERR_OB_INVALID_SEQ_ID));
        }
        self.advance_seq_id(order.seq_id);

        if order.post_only {
            return self.post_order(order);
        }

        match (order.order_type, order.time_in_force) {
            (OrderType::Limit, TimeInForce::GTK)
            | (OrderType::Market, TimeInForce::IOC)
            | (OrderType::Market, TimeInForce::FOK) => self.match_basic_order_by_qty(MakerOrder {
                order: order,
                match_state: MatchState {
                    remain_qty: Decimal::ZERO, // 作为taker，为0
                    filled_qty: Decimal::ZERO,
                },
                order_state: OrderState::New,
            }),
            _ => Err(OrderBookErr::new(err_code::ERR_OB_ORDER_TYPE_TIF)), // 未实现其他类型
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
            last_seq_id: self.seq_id,
        })
    }

    fn take_snapshot(&self) -> Result<OrderBookSnapshot, OrderBookErr> {
        Ok(OrderBookSnapshot {
            bid_orders: self.bid_orders.take_queue_snapshot(),
            ask_orders: self.ask_orders.take_queue_snapshot(),
            last_seq_id: self.seq_id,
        })
    }

    fn from_snapshot(s: OrderBookSnapshot) -> Result<OrderBook, OrderBookErr> {
        let n = s.ask_orders.len() + s.bid_orders.len();
        let mut order_id_to_key = HashMap::with_capacity(2 * n);
        for v in s.bid_orders.iter() {
            order_id_to_key.insert(
                v.order.order_id.clone(),
                OrderBookKey::new(v.order.price, v.order.seq_id),
            );
        }
        for v in s.ask_orders.iter() {
            order_id_to_key.insert(
                v.order.order_id.clone(),
                OrderBookKey::new(v.order.price, v.order.seq_id),
            );
        }

        let bid_orders = BTreeOrderQueue::from(s.bid_orders);
        let ask_orders = BTreeOrderQueue::from(s.ask_orders);

        Ok(OrderBook {
            seq_id: s.last_seq_id,
            order_id_to_orderbook_key: order_id_to_key,
            bid_orders,
            ask_orders,
        })
    }
}
#[derive(Debug)]
pub(crate) struct OrderBookErr {
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
    use crate::common::types::{Direction, Order, OrderID, OrderState, OrderType, TimeInForce};
    use crate::match_engine::orderbook::{
        KeyExt, MatchResult, OrderBook, OrderBookErr, OrderBookKey, OrderBookRequestHandler,
        OrderBookSnapshotHandler,
    };
    use rust_decimal::Decimal;
    use rust_decimal::prelude::FromPrimitive;

    fn post_limit_order(ob: &mut OrderBook, direction: Direction, price: f64, qty: f64) {
        let order = Order {
            client_origin_id: String::new(),
            post_only: true, // essential
            order_id: OrderID::new(),
            seq_id: ob.seq_id + 1,
            prev_seq_id: ob.seq_id,
            direction: direction,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GTK,
            price: Decimal::from_f64(price).unwrap(),
            target_qty: Decimal::from_f64(qty).unwrap(),
        };
        _ = ob.place_order(order)
    }

    fn add_limit_order(
        ob: &mut OrderBook,
        direction: Direction,
        price: f64,
        qty: f64,
    ) -> Result<MatchResult, OrderBookErr> {
        let order = Order {
            client_origin_id: String::new(),
            post_only: false,
            order_id: OrderID::new(),
            seq_id: ob.seq_id + 1,
            prev_seq_id: ob.seq_id,
            direction: direction,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GTK,
            price: Decimal::from_f64(price).unwrap(),
            target_qty: Decimal::from_f64(qty).unwrap(),
        };
        ob.place_order(order)
    }

    #[test]
    fn test_key_ext() {
        let k1 = KeyExt::Bid(OrderBookKey {
            price: Decimal::from_f64(101.0).unwrap(),
            seq_id: 1,
        });
        let k2 = KeyExt::Bid(OrderBookKey {
            price: Decimal::from_f64(100.0).unwrap(),
            seq_id: 2,
        });
        assert!(k1 < k2);

        let k3 = KeyExt::Ask(OrderBookKey {
            price: Decimal::from_f64(100.0).unwrap(),
            seq_id: 1,
        });
        let k4 = KeyExt::Ask(OrderBookKey {
            price: Decimal::from_f64(101.0).unwrap(),
            seq_id: 2,
        });
        assert!(k3 < k4);
    }

    #[test]
    fn test_limit_buy_order_filled() {
        let mut ob = OrderBook::new();
        // ask_q: (100.0, 10.0), (101.0, 5.0), (102.0, 20.0)
        // bid: (102, 30.0)
        // match: (100.0,10.0), (101.0,5.0), (102.0,15.0)
        // ask_q_new: (102.0, 5.0)
        post_limit_order(&mut ob, Direction::Sell, 100.0, 10.0);
        post_limit_order(&mut ob, Direction::Sell, 101.0, 5.0);
        post_limit_order(&mut ob, Direction::Sell, 102.0, 20.0);
        let result = add_limit_order(&mut ob, Direction::Buy, 102.0, 30.0).unwrap();
        assert_eq!(result.results.len(), 3);
        assert_eq!(result.results[0].price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(result.results[0].qty, Decimal::from_f64(10.0).unwrap());
        assert_eq!(result.results[1].price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(result.results[1].qty, Decimal::from_f64(5.0).unwrap());
        assert_eq!(result.results[2].price, Decimal::from_f64(102.0).unwrap());
        assert_eq!(result.results[2].qty, Decimal::from_f64(15.0).unwrap());

        let (_, peek) = ob.ask_orders.peek().unwrap();
        assert_eq!(peek.order.price, Decimal::from_f64(102.0).unwrap());
        assert_eq!(peek.match_state.remain_qty, Decimal::from_f64(5.0).unwrap());

        assert_eq!(result.order_state, OrderState::Filled);
    }

    #[test]
    fn test_limit_buy_order_partially_filled() {
        let mut ob = OrderBook::new();
        // ask_q: (100.0, 10.0), (101.0, 5.0), (102.0, 20.0)
        // bid: (101, 30.0)
        // match: (100.0,10.0), (101.0,5.0)
        // ask_q_new: (102.0, 5.0)
        // bid_q_new: (102.0,15.0)
        post_limit_order(&mut ob, Direction::Sell, 100.0, 10.0);
        post_limit_order(&mut ob, Direction::Sell, 101.0, 5.0);
        post_limit_order(&mut ob, Direction::Sell, 102.0, 20.0);
        let result = add_limit_order(&mut ob, Direction::Buy, 101.0, 30.0).unwrap();
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.results[0].price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(result.results[0].qty, Decimal::from_f64(10.0).unwrap());
        assert_eq!(result.results[1].price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(result.results[1].qty, Decimal::from_f64(5.0).unwrap());

        let (_, ask_peek) = ob.ask_orders.peek().unwrap();
        assert_eq!(ask_peek.order.price, Decimal::from_f64(102.0).unwrap());
        assert_eq!(
            ask_peek.match_state.remain_qty,
            Decimal::from_f64(20.0).unwrap()
        );

        let (_, bid_peek) = ob.bid_orders.peek().unwrap();
        assert_eq!(bid_peek.order.price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(
            bid_peek.match_state.filled_qty,
            Decimal::from_f64(15.0).unwrap(),
        );
        assert_eq!(
            bid_peek.match_state.remain_qty,
            Decimal::from_f64(15.0).unwrap(),
        );
        assert_eq!(bid_peek.order_state, OrderState::PartiallyFilled);
    }

    #[test]
    fn test_limit_sell_order_filled() {
        let mut ob = OrderBook::new();
        // bid_q: (102.0, 10.0), (101.0, 5.0), (100.0, 20.0)
        // ask: (100, 30.0)
        // match:(102.0,10.0), (101.0,5.0), (100.0,15.0)
        // bid_q_new: (100.0, 5.0)
        post_limit_order(&mut ob, Direction::Buy, 102.0, 10.0);
        post_limit_order(&mut ob, Direction::Buy, 101.0, 5.0);
        post_limit_order(&mut ob, Direction::Buy, 100.0, 20.0);
        let result = add_limit_order(&mut ob, Direction::Sell, 100.0, 30.0).unwrap();
        assert_eq!(result.results.len(), 3);
        assert_eq!(result.results[0].price, Decimal::from_f64(102.0).unwrap());
        assert_eq!(result.results[0].qty, Decimal::from_f64(10.0).unwrap());
        assert_eq!(result.results[1].price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(result.results[1].qty, Decimal::from_f64(5.0).unwrap());
        assert_eq!(result.results[2].price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(result.results[2].qty, Decimal::from_f64(15.0).unwrap());

        let (_, peek) = ob.bid_orders.peek().unwrap();
        assert_eq!(peek.order.price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(peek.match_state.remain_qty, Decimal::from_f64(5.0).unwrap());
    }

    #[test]
    fn test_limit_sell_order_partially_filled() {
        let mut ob = OrderBook::new();
        // bid_q: (102.0, 10.0), (101.0, 5.0), (100.0, 20.0)
        // ask: (101, 30.0)
        // match:(102.0,10.0), (101.0,5.0)
        // bid_q_new: (100.0,20.0)
        // ask_q_new: (101.0,15.0)

        post_limit_order(&mut ob, Direction::Buy, 102.0, 10.0);
        post_limit_order(&mut ob, Direction::Buy, 101.0, 5.0);
        post_limit_order(&mut ob, Direction::Buy, 100.0, 20.0);
        let result = add_limit_order(&mut ob, Direction::Sell, 101.0, 30.0).unwrap();
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.results[0].price, Decimal::from_f64(102.0).unwrap());
        assert_eq!(result.results[0].qty, Decimal::from_f64(10.0).unwrap());
        assert_eq!(result.results[1].price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(result.results[1].qty, Decimal::from_f64(5.0).unwrap());

        let (_, bid_peek) = ob.bid_orders.peek().unwrap();
        assert_eq!(bid_peek.order.price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(
            bid_peek.match_state.remain_qty,
            Decimal::from_f64(20.0).unwrap()
        );
        // PS: 也就是一次都没有成交过; 只要有一次成交即是PartiallyFilled
        assert_eq!(bid_peek.order_state, OrderState::New);

        let (_, ask_peek) = ob.ask_orders.peek().unwrap();
        assert_eq!(ask_peek.order.price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(
            ask_peek.match_state.remain_qty,
            Decimal::from_f64(15.0).unwrap()
        );
        assert_eq!(ask_peek.order_state, OrderState::PartiallyFilled);

        // bid_q: (100.0, 20.0)
        // ask_q: (101.0,15.0)
        // ask: (100, 2.0)
        // match:(100, 5.0)
        // bid_q_new: (100.0, 18.0)
        // ask_q_new: (101.0,15.0)
        let result2 = add_limit_order(&mut ob, Direction::Sell, 100.0, 2.0).unwrap();
        assert_eq!(result2.results.len(), 1);
        assert_eq!(result2.results[0].price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(result2.results[0].qty, Decimal::from_f64(2.0).unwrap());
        let (_, bid_peek2) = ob.bid_orders.peek().unwrap();
        assert_eq!(bid_peek2.order.price, Decimal::from_f64(100.0).unwrap());
        assert_eq!(
            bid_peek2.match_state.remain_qty,
            Decimal::from_f64(18.0).unwrap(),
        );
        assert_eq!(bid_peek2.order_state, OrderState::PartiallyFilled);

        let (_, ask_peek2) = ob.ask_orders.peek().unwrap();
        assert_eq!(ask_peek2.order.price, Decimal::from_f64(101.0).unwrap());
        assert_eq!(
            ask_peek2.match_state.remain_qty,
            Decimal::from_f64(15.0).unwrap(),
        );
    }

    #[test]
    fn test_snapshot() {
        let mut ob = OrderBook::new();
        post_limit_order(&mut ob, Direction::Buy, 102.0, 10.0);
        post_limit_order(&mut ob, Direction::Buy, 101.0, 5.0);
        post_limit_order(&mut ob, Direction::Buy, 100.0, 20.0);
        post_limit_order(&mut ob, Direction::Sell, 101.0, 30.0);

        let snapshot = ob.take_snapshot_with_depth(2).unwrap();
        assert_eq!(
            vec![
                (Decimal::from(102), Decimal::from(10)),
                (Decimal::from(101), Decimal::from(5)),
            ],
            snapshot.bid_orders,
        );
        assert_eq!(
            vec![(Decimal::from(101), Decimal::from(30)),],
            snapshot.ask_orders,
        );
        assert_eq!(4, snapshot.last_seq_id)
    }
}
