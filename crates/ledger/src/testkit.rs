#![allow(dead_code)]
use crate::common::types::Order;
use crate::ledger::spot::{SpotChangeResult, SpotLedger, SpotLedgerRPCHandler};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;

pub fn add_balance(l: &mut SpotLedger, account_id: u64, currency: &str, amount: f64) {
    l.add_deposit(account_id, currency, Decimal::from_f64(amount).unwrap())
        .unwrap();
}

pub fn place_orders(
    ledger: &mut SpotLedger,
    orders: impl Into<Vec<Order>>,
    amounts: Vec<Decimal>,
) -> Vec<SpotChangeResult> {
    let mut results = Vec::new();
    for (order, amount) in orders.into().into_iter().zip(amounts.into_iter()) {
        let res = ledger.place_order(order, amount).unwrap();
        results.push(res);
    }
    results
}
