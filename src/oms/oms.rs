//!
//! OMS(Order Management System) module.
//!

use crate::common::types::*;
use crate::pbcode::oms;
use std::collections::BTreeMap;
use std::fmt::Error;

struct AccountOrderList {
    bid_orders: BTreeMap<OrderID, OrderDetail>,
    ask_orders: BTreeMap<OrderID, OrderDetail>,
}

pub struct OMS {
    // symbol -> SymbolOMS
    symbols_oms: BTreeMap<Symbol, SymbolOMS>,
}

struct SymbolOMS {
    symbol: Symbol,
    // account_id -> bid \ ask orders
    active_orders: BTreeMap<u64, AccountOrderList>,
}

struct OrderChecker {}

impl OrderChecker {
    fn check_order(&self, order: &OrderDetail) -> Result<(), OrderCheckErr> {
        Ok(())
    }

    // MarketOrder: 同一账户下不能有相反方向
    fn check_self_trade(&self, order: &OrderDetail) -> Result<(), OrderCheckErr> {
        Ok(())
    }
}

pub struct OrderCheckErr {
    err_code: i32,
    self_trade_order_id: Option<OrderID>,
}
