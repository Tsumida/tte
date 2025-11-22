#![allow(dead_code)]

use rust_decimal::Decimal;

use crate::pbcode::oms::BizAction;

use crate::common::types::*; // 假设你上面的结构体定义放在 types.rs

/// 模拟下单
pub fn new_limit_order(
    order_id: &str,
    account_id: u64,
    direction: Direction,
    price: Decimal,
    qty: Decimal,
) -> Order {
    Order {
        order_id: order_id.to_string(),
        account_id,
        client_order_id: format!("CLIENT_{}", order_id),
        seq_id: 1,
        prev_seq_id: 0,
        time_in_force: TimeInForce::Gtk,
        order_type: OrderType::Limit,
        direction,
        price,
        target_qty: qty,
        post_only: false,
        trade_pair: TradePair {
            base: "BTC".to_string(),
            quote: "USDT".to_string(),
        },
    }
}

/// 模拟买单成交
pub fn fill_buy_limit_order(buy: &Order, sell: &Order, qty: Decimal) -> MatchResult {
    let symbol = buy.trade_pair.clone();
    let mut match_records = vec![];

    // 第一次成交 0.5 BTC
    match_records.push(MatchRecord {
        seq_id: 2,
        prev_seq_id: 1,
        match_id: 1,
        prev_match_id: 0,
        price: sell.price,
        qty: qty,
        direction: Direction::Buy,
        taker_order_id: buy.order_id.clone(),
        taker_account_id: buy.account_id,
        taker_state: OrderState::PartiallyFilled,
        maker_order_id: sell.order_id.clone(),
        maker_account_id: sell.account_id,
        maker_state: OrderState::PartiallyFilled,
        is_taker_fulfilled: false,
        is_maker_fulfilled: false,
        trade_pair: symbol.clone(),
    });

    let fill_result = FillOrderResult {
        original_order: buy.clone(),
        trade_pair: symbol.clone(),
        results: match_records,
        order_state: OrderState::Filled,
        total_filled_qty: qty,
    };

    MatchResult {
        action: BizAction::FillOrder,
        fill_result: Some(fill_result),
        cancel_result: None,
    }
}

// 模拟撤单
pub fn cancel_buy_limit_order(buy: &Order) -> MatchResult {
    let symbol = buy.trade_pair.clone();
    let cancel_result = CancelOrderResult {
        is_cancel_success: true,
        err_msg: None,
        direction: Direction::Buy,
        order_id: buy.order_id.clone(),
        account_id: buy.account_id,
        trade_pair: symbol.clone(),
        order_state: OrderState::Cancelled,
    };
    MatchResult {
        action: BizAction::CancelOrder,
        fill_result: None,
        cancel_result: Some(cancel_result),
    }
}
