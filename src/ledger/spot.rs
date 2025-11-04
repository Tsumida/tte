//!
//!
//!

// allow dead code for incomplete implementation
#![allow(dead_code)]

use std::{cmp::max, collections::BTreeMap};

use rust_decimal::Decimal;

use crate::{
    common::types::{Currency, Direction, MatchRecord, MatchResult, SeqID},
    pbcode::oms,
};

trait SpotLedgerRPCHandler {
    fn place_order(&mut self) {}
}

trait SpotLedgerMatchResultConsumer {
    fn fill_order(
        &mut self,
        match_record: &MatchRecord,
    ) -> Result<Vec<SpotChangeResult>, SpotLedgerErr>;

    fn replace_order(
        &mut self,
        match_result: &MatchResult,
    ) -> Result<SpotChangeResult, SpotLedgerErr>;

    fn cancel_order(
        &mut self,
        match_result: &MatchResult,
    ) -> Result<SpotChangeResult, SpotLedgerErr>;
}

#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
struct SpotChangeResult {
    is_action_success: bool,
    action_failed_err: String,
    spot_event_id: u64,
    transfer_id: u64,
    action: i32,
    spot_before: Spot,
    spot_after: Spot,
    // transfer_frozen_receipts_snapshot: Vec<(String, FrozenReceipt)>, // 主要是debug
}
// 记录单次流程中的冻结额度，主要用于整体释放。
// 场景：限价单撤单时要按orderID释放整个冻结额度
#[derive(Clone)]
struct FrozenReceipt {
    frozen_id: String, // 对应order_id, withdraw_id, transfer_id等
    account_id: u64,
    currency: Currency,
    remain_frozen: Decimal,
    target_frozen: Decimal, // 初始冻结额度, 在下单、改单时更新。
}

struct SpotLedger {
    spots: BTreeMap<u64, BTreeMap<Currency, Spot>>, // account_id -> currency -> Spot
    order_frozen_receipts: BTreeMap<String, FrozenReceipt>, // frozen_id -> FrozenReceipt
    withdraw_frozen_receipts: BTreeMap<String, FrozenReceipt>,
    transfer_frozen_receipts: BTreeMap<String, FrozenReceipt>,
    last_seq_id: SeqID,
}

// 所有更新方法必须要符合CAS原则，只有必定成功才更新状态;否则就输出失败事件。
// 对于失败事件，通过冲正来修复。
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

    pub fn get_spot(&self, account_id: u64, currency: &str) -> Option<&Spot> {
        self.spots
            .get(&account_id)
            .and_then(|currency_map| currency_map.get(currency))
    }

    pub fn get_all_balances(&self, account_id: u64) -> BTreeMap<&Currency, &Spot> {
        match self.spots.get(&account_id) {
            Some(currency_map) => currency_map.iter().collect(),
            None => BTreeMap::new(),
        }
    }

    // 新增Spot，并返回变化前的Spot
    fn add_deposit(
        &mut self,
        account_id: u64,
        currency: &str,
        amount: Decimal,
    ) -> Result<(Spot, Spot), SpotLedgerErr> {
        let spot = self
            .spots
            .entry(account_id)
            .or_insert_with(BTreeMap::new)
            .entry(currency.to_string())
            .or_insert_with(|| Spot::new(account_id, currency.to_string()));

        let before = spot.clone();

        spot.deposit += amount;
        Ok((before, spot.clone()))
    }

    // 按order_id创建冻结额度，并返回变化后的Spot和FrozenReceipt
    fn freeze(
        &mut self,
        account_id: u64,
        currency: &str,
        amount: Decimal,
        frozen_id: &str,
    ) -> Result<(Spot, FrozenReceipt), SpotLedgerErr> {
        let spot = self
            .spots
            .get_mut(&account_id)
            .and_then(|currency_map| currency_map.get_mut(currency))
            .ok_or(SpotLedgerErr {
                code: 1,
                msg: format!(
                    "Spot not found for account_id {}, currency {}",
                    account_id, currency
                ),
            })?;

        if spot.deposit - spot.frozen < amount {
            return Err(SpotLedgerErr {
                code: 2,
                msg: format!(
                    "Insufficient balance to freeze for account_id {}, currency {}",
                    account_id, currency
                ),
            });
        }

        spot.frozen += amount;

        let receipt = self
            .order_frozen_receipts
            .entry(frozen_id.to_string())
            .or_insert(FrozenReceipt {
                frozen_id: frozen_id.to_string(),
                account_id,
                currency: currency.to_string(),
                remain_frozen: Decimal::ZERO,
                target_frozen: Decimal::ZERO,
            });

        receipt.remain_frozen += amount;
        receipt.target_frozen += amount;

        Ok((spot.clone(), receipt.clone()))
    }

    // 按froze_id释放一定的冻结额度, 并返回变化后的Spot和FrozenReceipt
    fn release_frozen_amount(
        &mut self,
        frozen_id: &str,
        amount: Decimal,
    ) -> Result<(Spot, FrozenReceipt), SpotLedgerErr> {
        let receipt = self
            .order_frozen_receipts
            .get_mut(frozen_id)
            .ok_or(SpotLedgerErr {
                code: 1,
                msg: format!("FrozenReceipt not found for frozen_id {}", frozen_id),
            })?;

        if receipt.remain_frozen < amount {
            return Err(SpotLedgerErr {
                code: 2,
                msg: format!(
                    "Insufficient frozen amount to release for frozen_id {}",
                    frozen_id
                ),
            });
        }

        let spot = self
            .spots
            .get_mut(&receipt.account_id)
            .and_then(|currency_map| currency_map.get_mut(&receipt.currency))
            .ok_or(SpotLedgerErr {
                code: 3,
                msg: format!(
                    "Spot not found for account_id {}, currency {}",
                    receipt.account_id, receipt.currency
                ),
            })?;

        spot.frozen -= amount;
        receipt.remain_frozen -= amount;

        Ok((spot.clone(), receipt.clone()))
    }

    // 按frozen_id释放全部冻结额度, 并返回变化后的Spot和FrozenReceipt
    fn release_all(&mut self, frozen_id: &str) -> Result<(Spot, FrozenReceipt), SpotLedgerErr> {
        let receipt = self
            .order_frozen_receipts
            .get_mut(frozen_id)
            .ok_or(SpotLedgerErr {
                code: 1,
                msg: format!("FrozenReceipt not found for frozen_id {}", frozen_id),
            })?;

        let amount = receipt.remain_frozen;

        let spot = self
            .spots
            .get_mut(&receipt.account_id)
            .and_then(|currency_map| currency_map.get_mut(&receipt.currency))
            .ok_or(SpotLedgerErr {
                code: 3,
                msg: format!(
                    "Spot not found for account_id {}, currency {}",
                    receipt.account_id, receipt.currency
                ),
            })?;

        spot.frozen -= amount;
        receipt.remain_frozen = Decimal::ZERO;

        Ok((spot.clone(), receipt.clone()))
    }
}

impl SpotLedgerMatchResultConsumer for SpotLedger {
    // 对taker和maker 依次执行
    fn fill_order(
        &mut self,
        match_result: &MatchRecord,
    ) -> Result<Vec<SpotChangeResult>, SpotLedgerErr> {
        let taker_order_id = match_result.taker_order_id.clone();
        let maker_order_id = match_result.maker_order_id.clone();

        let mut taker = self
            .order_frozen_receipts
            .get_mut(&taker_order_id)
            .cloned()
            .ok_or(SpotLedgerErr {
                code: 1,
                msg: format!("Taker order_id {} not found", taker_order_id),
            })?;

        let mut maker = self
            .order_frozen_receipts
            .get_mut(&maker_order_id)
            .cloned()
            .ok_or(SpotLedgerErr {
                code: 1,
                msg: format!("Maker order_id {} not found", maker_order_id),
            })?;

        // 根据买卖方向，更新amount.
        let (taker_amount, maker_amount) = match match_result.direction {
            Direction::Buy => (match_result.qty * match_result.price, match_result.qty),
            Direction::Sell => (match_result.qty, match_result.qty * match_result.price),
        };
        taker.remain_frozen -= taker_amount; // todo: 如果amount为负数, release会导致资产虚增，应该检查。
        maker.remain_frozen -= maker_amount;

        let taker_spot = self
            .spots
            .get_mut(&taker.account_id)
            .and_then(|currency_map| currency_map.get_mut(&taker.currency))
            .ok_or(SpotLedgerErr {
                code: 2,
                msg: format!(
                    "Taker spot not found for account_id {}, currency {}",
                    taker.account_id, taker.currency
                ),
            })?;
        let taker_before = taker_spot.clone();
        taker_spot.frozen -= taker_amount;
        taker_spot.deposit += taker_amount;
        let taker_log = SpotChangeResult {
            is_action_success: true,
            action_failed_err: "".to_string(),
            spot_event_id: 0,
            transfer_id: 0,
            action: oms::BizAction::FillOrder as i32,
            spot_before: taker_before,
            spot_after: taker_spot.clone(),
        };

        let maker_spot = self
            .spots
            .get_mut(&maker.account_id)
            .and_then(|currency_map| currency_map.get_mut(&maker.currency))
            .ok_or(SpotLedgerErr {
                code: 2,
                msg: format!(
                    "Maker spot not found for account_id {}, currency {}",
                    maker.account_id, maker.currency
                ),
            })?;
        let maker_before = maker_spot.clone();
        maker_spot.frozen -= maker_amount;
        maker_spot.deposit += maker_amount;

        let maker_log = SpotChangeResult {
            is_action_success: true,
            action_failed_err: "".to_string(),
            spot_event_id: 0,
            transfer_id: 0,
            action: oms::BizAction::FillOrder as i32,
            spot_before: maker_before,
            spot_after: maker_spot.clone(),
        };

        // 这里必定成功，更新Spot状态
        if !match_result.is_taker_fulfilled {
            _ = self
                .order_frozen_receipts
                .insert(taker_order_id.clone(), taker);
        } else {
            _ = self.order_frozen_receipts.remove(&taker_order_id);
        }
        if !match_result.is_maker_fulfilled {
            _ = self
                .order_frozen_receipts
                .insert(maker_order_id.clone(), maker);
        } else {
            _ = self.order_frozen_receipts.remove(&maker_order_id);
        }

        // 生成spot_log
        Ok(vec![taker_log, maker_log])
    }

    fn replace_order(
        &mut self,
        match_result: &MatchResult,
    ) -> Result<SpotChangeResult, SpotLedgerErr> {
        let result = match_result.replace_result.as_ref().ok_or(SpotLedgerErr {
            code: 3,
            msg: "No replace_result in MatchResult".to_string(),
        })?;

        if !result.is_replace_success {
            return Err(SpotLedgerErr {
                code: 5,
                msg: format!(
                    "Replace order failed: {}",
                    result.err_msg.clone().unwrap_or_default()
                ),
            });
        }

        if result.order_state.is_final() {
            return Err(SpotLedgerErr {
                code: 4,
                msg: "Replace order result is in final state".to_string(),
            });
        }

        let (currency, new_frozen): (&str, Decimal) = match result.direction {
            Direction::Buy => (
                result.symbol.quote.as_ref(),
                result.new_price * result.new_quantity,
            ),
            Direction::Sell => (result.symbol.base.as_ref(), result.new_quantity),
        };

        let order = self
            .order_frozen_receipts
            .get_mut(&result.order_id)
            .ok_or(SpotLedgerErr {
                code: 1,
                msg: format!("Order_id {} not found", result.order_id),
            })?;

        let spot = self
            .spots
            .get_mut(&result.account_id)
            .and_then(|currency_map| currency_map.get_mut(currency))
            .ok_or(SpotLedgerErr {
                code: 2,
                msg: format!(
                    "Spot not found for account_id {},or {}",
                    result.account_id, result.order_id
                ),
            })?;

        // update frozen & receipt
        let frozen_delta = max(Decimal::ZERO, new_frozen - order.target_frozen);
        spot.frozen -= frozen_delta;
        order.target_frozen = new_frozen;
        order.remain_frozen -= frozen_delta; // 保证 target_frozen - remain_frozen = 已成交量不变

        Ok(SpotChangeResult {
            is_action_success: true,
            action_failed_err: "".to_string(),
            spot_event_id: 0,
            transfer_id: 0,
            action: oms::BizAction::ReplaceOrder as i32,
            spot_before: spot.clone(),
            spot_after: spot.clone(),
        })
    }

    fn cancel_order(
        &mut self,
        match_result: &MatchResult,
    ) -> Result<SpotChangeResult, SpotLedgerErr> {
        let result = match_result.cancel_result.as_ref().ok_or(SpotLedgerErr {
            code: 3,
            msg: "No cancel_result in MatchResult".to_string(),
        })?;

        if !result.is_cancel_success {
            return Err(SpotLedgerErr {
                code: 5,
                msg: format!(
                    "Cancel order failed: {}",
                    result.err_msg.clone().unwrap_or_default()
                ),
            });
        }

        let currency: &str = match result.direction {
            Direction::Buy => result.symbol.quote.as_ref(),
            Direction::Sell => result.symbol.base.as_ref(),
        };

        // delete receipts
        let order = self
            .order_frozen_receipts
            .remove(&result.order_id)
            .ok_or(SpotLedgerErr {
                code: 1,
                msg: format!("Order_id {} not found", result.order_id),
            })?;

        // release frozen
        let spot = self
            .spots
            .get_mut(&result.account_id)
            .and_then(|currency_map| currency_map.get_mut(currency))
            .ok_or(SpotLedgerErr {
                code: 2,
                msg: format!(
                    "Spot not found for account_id {},or {}",
                    result.account_id, result.order_id
                ),
            })?;
        let spot_before = spot.clone();
        let remain_frozen = order.remain_frozen;
        spot.frozen -= remain_frozen;

        Ok(SpotChangeResult {
            is_action_success: true,
            action_failed_err: "".to_string(),
            spot_event_id: 0,
            transfer_id: 0,
            action: oms::BizAction::ReplaceOrder as i32,
            spot_before: spot_before,
            spot_after: spot.clone(),
        })
    }
}

impl SpotLedgerRPCHandler for SpotLedger {
    fn place_order(&mut self) {}
}

#[derive(Debug, Clone)]
pub struct SpotLedgerErr {
    code: i32,
    msg: String,
}

impl std::fmt::Display for SpotLedgerErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SpotLedgerErr code: {}, msg: {}", self.code, self.msg)
    }
}

#[allow(dead_code)]
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::FromPrimitive;

    fn add_balance(l: &mut SpotLedger, account_id: u64, currency: &str, amount: f64) {
        l.add_deposit(account_id, currency, Decimal::from_f64(amount).unwrap())
            .unwrap();
    }

    #[test]
    fn test_place_order() {
        let mut ledger = SpotLedger::new();
        add_balance(&mut ledger, 1001, "BTC", 1.0);
        add_balance(&mut ledger, 1001, "USD", 10000.0);
        add_balance(&mut ledger, 1001, "USD", 2000.0);

        let balance = ledger.get_all_balances(1001);
        assert!(
            balance.get(&"BTC".to_string()).unwrap().deposit == Decimal::from_f64(1.0).unwrap()
        );
        assert!(
            balance.get(&"USD".to_string()).unwrap().deposit == Decimal::from_f64(12000.0).unwrap()
        );
    }

    #[test]
    fn test_freeze_release() {
        let mut ledger = SpotLedger::new();
        add_balance(&mut ledger, 1001, "BTC", 1.0);
        add_balance(&mut ledger, 1001, "USD", 10000.0);
        add_balance(&mut ledger, 1001, "USD", 2000.0);
    }
}
