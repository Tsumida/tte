//!
//! OMS(Order Management System) module.
//!

#![allow(dead_code)]

use rust_decimal::Decimal;

use crate::common::err_code;
use crate::common::types::*;
use crate::pbcode::oms;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

struct AccountOrderList {
    bid_orders: BTreeMap<OrderID, OrderDetail>,
    ask_orders: BTreeMap<OrderID, OrderDetail>,
}

pub struct OMS {
    symbol: Symbol,
    active_orders: BTreeMap<u64, AccountOrderList>, // account_id -> bid \ ask orders
    symbol_configs: oms::SymbolConfig,
    client_order_map: HashMap<String, OrderID>, // client_order_id -> order_id
    last_price: Decimal,                        // 交易对最新成交价
    last_seq_id: SeqID,
}

impl OMS {
    fn check_place_order(&self, order: &OrderDetail) -> Result<(), OrderCheckErr> {
        self.check_symbol_trading()?;
        self.check_price_in_range(order.price())?;
        self.check_qty(order.quantity())?;
        self.check_client_order_id(order.order_id())?;
        Ok(())
    }

    fn check_symbol_trading(&self) -> Result<(), OrderCheckErr> {
        if self.symbol_configs.state != oms::SymbolState::SymbolTrading as i32 {
            return Err(OrderCheckErr {
                err_code: err_code::ERR_OMS_SYMBOL_NOT_TRADING,
            });
        }
        Ok(())
    }

    fn check_price_in_range(&self, price: Decimal) -> Result<(), OrderCheckErr> {
        let (min_price, max_price) = (
            self.last_price
                * (Decimal::ONE
                    - Decimal::from_str(&self.symbol_configs.volatility_limit).unwrap()),
            self.last_price
                * (Decimal::ONE
                    + Decimal::from_str(&self.symbol_configs.volatility_limit).unwrap()),
        );
        if price < min_price || price > max_price {
            return Err(OrderCheckErr {
                err_code: err_code::ERR_OMS_PRICE_OUT_OF_RANGE,
            });
        }
        Ok(())
    }

    fn check_qty(&self, qty: Decimal) -> Result<(), OrderCheckErr> {
        if qty < Decimal::from_str(&self.symbol_configs.min_quantity_increment).unwrap() {
            return Err(OrderCheckErr {
                err_code: err_code::ERR_OMS_QTY_OUT_OF_RANGE,
            });
        }
        Ok(())
    }

    fn check_client_order_id(&self, client_order_id: &str) -> Result<(), OrderCheckErr> {
        if !client_order_id.is_empty() && self.client_order_map.contains_key(client_order_id) {
            return Err(OrderCheckErr {
                err_code: err_code::ERR_OMS_DUPLICATE_PLACE,
            });
        }

        Ok(())
    }
}

pub struct OrderCheckErr {
    err_code: i32,
}

pub type OMSSnapshot = OMS;
