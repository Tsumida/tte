//!
//!
//!

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::{
    common::types::{Currency, Direction, MatchRecord, SeqID},
    match_engine::orderbook::MatchResult,
};

pub struct Spot {
    account_id: u64,
    currency: Currency,
    deposit: Decimal,
    frozen: Decimal,
}

impl Spot {
    pub fn new(account_id: u64, currency: Currency) -> Self {
        Spot {
            account_id,
            currency,
            deposit: Decimal::ZERO,
            frozen: Decimal::ZERO,
        }
    }

    fn check(&self) -> bool {
        // 0 <= frozen <= deposit
        // deposit >= 0
        self.frozen >= Decimal::ZERO && self.deposit >= Decimal::ZERO && self.frozen <= self.deposit
    }
}

// 单账户的币种余额变动流水
struct SpotLog {
    account_id: u64,
    currency: Currency,
    before: Decimal, // 变动前余额
    delta: Decimal,  // 余额变动量，正为增加，负为减少
    after: Decimal,  // 变动后余额
    action: String,  // 对应SpotChangeReason

    // 其他关联信息
    transfer_id: Option<String>,
    deposit_id: Option<String>,
    withdraw_id: Option<String>,
    order_id: Option<String>,
    fill_id: Option<String>, // 对应每次触发成交的SeqID
}

// 记录单次流程中的冻结额度，主要用于整体释放。
// 场景：限价单撤单时要按orderID释放整个冻结额度
#[derive(Clone)]
struct FrozenReceipt {
    frozen_id: String, // 对应order_id, withdraw_id, transfer_id等
    account_id: u64,
    currency: Currency,
    frozen_amount: Decimal,
}

struct SpotLedger {
    spots: BTreeMap<u64, BTreeMap<Currency, Spot>>, // account_id -> currency -> Spot
    order_frozen_receipts: BTreeMap<String, FrozenReceipt>, // frozen_id -> FrozenReceipt
    withdraw_frozen_receipts: BTreeMap<String, FrozenReceipt>,
    transfer_frozen_receipts: BTreeMap<String, FrozenReceipt>,
    last_seq_id: SeqID,
}

impl SpotLedger {
    pub fn new() -> Self {
        SpotLedger {
            spots: BTreeMap::new(),
            order_frozen_receipts: BTreeMap::new(),
            withdraw_frozen_receipts: BTreeMap::new(),
            transfer_frozen_receipts: BTreeMap::new(),
            last_seq_id: 0,
        }
    }

    // deposit -> frozen
    pub fn freeze(
        &mut self,
        account_id: u64,
        currency: Currency,
        amount: Decimal,
        frozen_id: String,
    ) -> Result<SpotLog, SpotLedgerErr> {
        todo!()
    }

    // frozen -> deposit
    pub fn release_amount(
        &mut self,
        account_id: u64,
        frozen_id: String,
        amount: Decimal,
    ) -> Result<SpotLog, SpotLedgerErr> {
        todo!()
    }

    // frozen -> deposit
    pub fn release_all(
        &mut self,
        account_id: u64,
        frozen_id: String,
    ) -> Result<SpotLog, SpotLedgerErr> {
        todo!()
    }

    pub fn fill_order(&mut self, match_result: MatchRecord) -> Result<Vec<SpotLog>, SpotLedgerErr> {
        let taker_order_id = match_result.taker_order_id;
        let maker_order_id = match_result.maker_order_id;

        let mut taker = self
            .order_frozen_receipts
            .get_mut(&taker_order_id)
            .cloned()
            .ok_or(SpotLedgerErr {
                code: 1,
                account_id: 0,
                currency: "USD".to_string(), // todo
                msg: format!("Taker order_id {} not found", taker_order_id),
            })?;

        let mut maker = self
            .order_frozen_receipts
            .get_mut(&maker_order_id)
            .cloned()
            .ok_or(SpotLedgerErr {
                code: 1,
                account_id: 0,
                currency: "USD".to_string(), // todo
                msg: format!("Maker order_id {} not found", maker_order_id),
            })?;

        // 根据买卖方向，更新amount.
        let (taker_amount, maker_amount) = match match_result.direction {
            Direction::Buy => (match_result.qty * match_result.price, match_result.qty),
            Direction::Sell => (match_result.qty, match_result.qty * match_result.price),
        };
        taker.frozen_amount -= taker_amount; // todo: 如果amount为负数, release会导致资产虚增，应该检查。
        maker.frozen_amount -= maker_amount;

        // 这里必定成功，更新Spot状态
        if !match_result.is_taker_fulfilled {
            _ = self
                .order_frozen_receipts
                .insert(taker_order_id.clone(), taker);
        }
        if !match_result.is_maker_fulfilled {
            _ = self
                .order_frozen_receipts
                .insert(maker_order_id.clone(), maker);
        }

        // 生成spot_log
        todo!()
    }
}

#[derive(Debug)]
pub struct SpotLedgerErr {
    code: i32,
    account_id: u64,
    currency: Currency,
    msg: String,
}
