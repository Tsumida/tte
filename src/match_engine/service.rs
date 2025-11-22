//#![allow(dead_code)]
#![deny(clippy::unwrap_used)]

use crate::match_engine::orderbook::OrderBook;

struct MatchEngineService {
    orderbook: OrderBook,
}
