#![allow(dead_code)]
use crate::spot::{SpotChangeResult, SpotLedger, SpotLedgerRPCHandler};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use tte_core::types::Order;

pub fn add_balance(l: &mut SpotLedger, account_id: u64, currency: &str, amount: f64) {
    l.add_deposit(account_id, currency, Decimal::from_f64(amount).unwrap())
        .unwrap();
}

pub fn place_orders(
    ledger: &mut SpotLedger,
    orders: impl Into<Vec<Order>>,
    amounts: Vec<Decimal>,
) -> Vec<SpotChangeResult> {
    let mut handlers = Vec::new();
    for (order, amount) in orders.into().into_iter().zip(amounts.into_iter()) {
        let h = ledger.place_order(order, amount).unwrap();
        handlers.push(h);
    }
    handlers.into_iter().map(|h| h(ledger)).collect()
}
