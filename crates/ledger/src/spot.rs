#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};

use getset::Getters;
use rust_decimal::Decimal;

use tte_core::{
    err_code::{self, ERR_INPOSSIBLE_STATE, TradeEngineErr},
    pbcode::oms::{BalanceEvent, BizAction},
    types::{CancelOrderResult, Direction, FillRecord, Order, Symbol},
};

pub trait SpotLedgerRPCHandler {
    fn place_order(
        &mut self,
        order: Order,
        amount: Decimal,
    ) -> Result<SpotChangeResult, SpotLedgerErr>;
}

pub trait SpotLedgerMatchResultConsumer {
    fn fill_order(
        &mut self,
        match_record: &FillRecord,
    ) -> Result<Vec<SpotChangeResult>, SpotLedgerErr>;

    fn cancel_order(
        &mut self,
        result: &CancelOrderResult,
    ) -> Result<SpotChangeResult, SpotLedgerErr>;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Spot {
    account_id: u64,
    currency: Symbol,
    deposit: Decimal,
    frozen: Decimal,
    update_time: u64,
}

impl Spot {
    pub fn new(account_id: u64, currency: Symbol) -> Self {
        Spot {
            account_id,
            currency,
            deposit: Decimal::ZERO,
            frozen: Decimal::ZERO,
            update_time: 0,
        }
    }

    fn check(&self) -> bool {
        // 0 <= frozen <= deposit
        // deposit >= 0
        self.frozen >= Decimal::ZERO && self.deposit >= Decimal::ZERO && self.frozen <= self.deposit
    }
}

// 单账户的币种余额变动流水
#[derive(Debug, Clone, Getters)]
pub struct SpotChangeResult {
    #[getset(get = "pub")]
    is_action_success: bool,
    action_failed_err: Option<String>,
    spot_id: u64,
    action: i32,
    spot_before: Spot,
    spot_after: Spot,
    // transfer_frozen_receipts_snapshot: Vec<(String, FrozenReceipt)>, // 主要是debug
}

impl SpotChangeResult {
    pub fn to_balance_event(&self) -> Option<BalanceEvent> {
        if !self.is_action_success {
            return None;
        }
        Some(BalanceEvent {
            account_id: self.spot_after.account_id,
            currency: self.spot_after.currency.clone(),
            deposit: self.spot_after.deposit.to_string(),
            frozen: self.spot_after.frozen.to_string(),
            balance: (self.spot_after.deposit - self.spot_after.frozen).to_string(),
            update_time: self.spot_after.update_time,
        })
    }
}

// 记录单次流程中的冻结额度，主要用于整体释放。
// 场景：限价单撤单时要按orderID释放整个冻结额度
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FrozenReceipt {
    frozen_id: String, // 对应order_id, withdraw_id, transfer_id等
    account_id: u64,
    currency: Symbol,
    remain_frozen: Decimal,
    target_frozen: Decimal, // 初始冻结额度, 在下单、改单时更新。
}

// 单币种状态修改
#[derive(Debug, Clone)]
pub struct SingleCurrencyTx {
    spot_id: u64,
    spot: Option<Spot>,
    frozen_receipt: Option<FrozenReceipt>,
}

trait SingleCurrencyTxUpdater {
    // deposit -> deposit + amount
    fn add_deposit(&mut self, amount: Decimal) -> Result<(), SpotLedgerErr>;

    // deposit -> deposit - amount
    fn sub_deposit(&mut self, amount: Decimal) -> Result<(), SpotLedgerErr>;

    // (deposit, None) -> (deposit - amount, Some(frozen_receipt))
    fn freeze(&mut self, frozen_id: &str, amount: Decimal) -> Result<(), SpotLedgerErr>;

    // (frozen, Some(frozen_receipt)) -> (frozen - amount, Some(frozen_receipt - amount))
    fn release_frozen(&mut self, amount: Decimal) -> Result<(), SpotLedgerErr>;

    // (frozen, Some(frozen_receipt)) -> (frozen - remain_frozen, None)
    fn release_all(&mut self) -> Result<(), SpotLedgerErr>;
}

impl SingleCurrencyTxUpdater for SingleCurrencyTx {
    fn add_deposit(&mut self, amount: Decimal) -> Result<(), SpotLedgerErr> {
        if self.spot.is_none() {
            return Err(SpotLedgerErr::new(
                err_code::ERR_INPOSSIBLE_STATE,
                "Spot is None when adding deposit",
            ));
        }
        self.spot.as_mut().unwrap().deposit += amount;
        Ok(())
    }

    fn sub_deposit(&mut self, amount: Decimal) -> Result<(), SpotLedgerErr> {
        if self.spot.is_none() {
            return Err(SpotLedgerErr::new(
                err_code::ERR_INPOSSIBLE_STATE,
                "Spot is None when subtracting deposit",
            ));
        }
        let spot = self.spot.as_mut().unwrap();
        if spot.deposit < amount {
            return Err(SpotLedgerErr::new(
                err_code::ERR_LEDGER_INSUFFICIENT_BALANCE,
                "Insufficient balance to subtract deposit",
            ));
        }
        spot.deposit -= amount;
        Ok(())
    }

    fn freeze(&mut self, frozen_id: &str, amount: Decimal) -> Result<(), SpotLedgerErr> {
        if self.frozen_receipt.is_some() {
            return Err(SpotLedgerErr::new(
                err_code::ERR_INPOSSIBLE_STATE,
                "FrozenReceipt already exists when freezing",
            ));
        }

        if let Some(spot) = self.spot.as_mut() {
            if spot.deposit - spot.frozen < amount {
                return Err(SpotLedgerErr::new(
                    err_code::ERR_LEDGER_INSUFFICIENT_BALANCE,
                    "Insufficient available balance to freeze",
                ));
            }
            spot.frozen += amount;
        }
        self.frozen_receipt = Some(FrozenReceipt {
            frozen_id: frozen_id.to_string(),
            account_id: self.spot.as_ref().unwrap().account_id,
            currency: self.spot.as_ref().unwrap().currency.clone(),
            remain_frozen: amount,
            target_frozen: amount,
        });
        Ok(())
    }

    fn release_frozen(&mut self, amount: Decimal) -> Result<(), SpotLedgerErr> {
        if let Some(receipt) = &mut self.frozen_receipt {
            if receipt.remain_frozen < amount {
                return Err(SpotLedgerErr::new(
                    err_code::ERR_LEDGER_INSUFFICIENT_FROZEN,
                    "Release amount exceeds remaining frozen amount",
                ));
            }
            let spot = self.spot.as_mut().unwrap();
            if spot.frozen < amount {
                return Err(SpotLedgerErr::new(
                    err_code::ERR_LEDGER_INSUFFICIENT_FROZEN,
                    "Insufficient frozen amount to release",
                ));
            }
            spot.frozen -= amount;
            receipt.remain_frozen -= amount;
            Ok(())
        } else {
            Err(SpotLedgerErr::new(
                ERR_INPOSSIBLE_STATE,
                "No FrozenReceipt to release from",
            ))
        }
    }

    fn release_all(&mut self) -> Result<(), SpotLedgerErr> {
        if let Some(receipt) = &mut self.frozen_receipt {
            let amount = receipt.remain_frozen;
            let spot = self.spot.as_mut().unwrap();
            if spot.frozen < amount {
                return Err(SpotLedgerErr::new(
                    err_code::ERR_INPOSSIBLE_STATE,
                    "Insufficient frozen amount to release all",
                ));
            }
            spot.frozen -= amount;
        }
        self.frozen_receipt = None;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpotLedger {
    spots: BTreeMap<u64, BTreeMap<Symbol, Spot>>, // account_id -> currency -> Spot
    order_frozen_receipts: BTreeMap<String, FrozenReceipt>, // frozen_id -> FrozenReceipt
    last_seq_id: u64,
    last_spot_id: u64, // 目前是sequencer commit后串行调用， 因此一个spot_id足够
}

// 所有更新方法必须要符合CAS原则，只有必定成功才更新内存状态;否则就输出失败事件。
// 对于失败事件，通过冲正来修复。
impl SpotLedger {
    pub fn new() -> Self {
        SpotLedger {
            last_spot_id: 0,
            spots: BTreeMap::new(),
            order_frozen_receipts: BTreeMap::new(),
            last_seq_id: 0,
        }
    }

    pub fn get_frozen_receipt(&self, frozen_id: &str) -> Option<&FrozenReceipt> {
        self.order_frozen_receipts.get(frozen_id)
    }

    pub fn get_spot(&self, account_id: u64, currency: &str) -> Option<&Spot> {
        self.spots
            .get(&account_id)
            .and_then(|currency_map| currency_map.get(currency))
    }

    pub fn get_spot_or_default(&self, account_id: u64, currency: &str) -> Spot {
        self.spots
            .get(&account_id)
            .and_then(|currency_map| currency_map.get(currency))
            .cloned()
            .unwrap_or(Spot::new(account_id, currency.to_string()))
    }

    // [](currency, deposit, frozen)
    pub fn get_balance(&self, account_id: u64) -> Vec<(Symbol, Decimal, Decimal, u64)> {
        match self.spots.get(&account_id) {
            Some(currency_map) => currency_map
                .iter()
                .map(|(currency, spot)| {
                    (
                        currency.clone(),
                        spot.deposit,
                        spot.frozen,
                        spot.update_time,
                    )
                })
                .collect(),
            None => Vec::new(),
        }
    }

    pub(crate) fn get_spot_and_frozen(
        &self,
        account_id: u64,
        currency: &str,
        frozen_id: &str,
    ) -> SingleCurrencyTx {
        SingleCurrencyTx {
            spot_id: self.current_spot_id(),
            // 对于首次更新的spot，直接创建默认spot; 否则commit (None, None)会失败
            spot: Some(self.get_spot_or_default(account_id, currency)),
            frozen_receipt: self.get_frozen_receipt(frozen_id).cloned(),
        }
    }

    pub fn commit(
        &mut self,
        spot_id: u64,
        action: i32,
        before: SingleCurrencyTx,
        after: SingleCurrencyTx,
    ) -> SpotChangeResult {
        // (None, Some) -> Insert
        // (Some, Some) -> Update
        // (Some, None) -> Delete
        match (&before.spot, &after.spot) {
            (_, Some(spot_after)) => {
                // Insert or Update
                self.spots
                    .entry(spot_after.account_id)
                    .or_insert_with(BTreeMap::new)
                    .insert(spot_after.currency.clone(), spot_after.clone());
            }
            (Some(spot_before), None) => {
                // Delete
                if let Some(currency_map) = self.spots.get_mut(&spot_before.account_id) {
                    currency_map.remove(&spot_before.currency);
                    if currency_map.is_empty() {
                        self.spots.remove(&spot_before.account_id);
                    }
                }
            }
            _ => {}
        }

        match (&before.frozen_receipt, &after.frozen_receipt) {
            (_, Some(receipt_after)) => {
                // Insert or Update
                self.order_frozen_receipts
                    .insert(receipt_after.frozen_id.clone(), receipt_after.clone());
            }
            (Some(receipt_before), None) => {
                // Delete
                self.order_frozen_receipts.remove(&receipt_before.frozen_id);
            }
            _ => {}
        }

        SpotChangeResult {
            is_action_success: true,
            action_failed_err: None,
            spot_id: spot_id,
            action,
            spot_before: before.spot.unwrap(),
            spot_after: after.spot.unwrap(),
        }
    }

    #[inline]
    fn current_spot_id(&self) -> u64 {
        self.last_spot_id
    }

    #[inline]
    fn advance_spot_id(&mut self) -> u64 {
        self.last_spot_id += 1;
        self.last_spot_id
    }

    fn get_all_balances(&self, account_id: u64) -> HashMap<&Symbol, &Spot> {
        match self.spots.get(&account_id) {
            Some(currency_map) => currency_map.iter().collect(),
            None => HashMap::new(),
        }
    }

    // 新增Spot，并返回变化前的Spot
    pub fn add_deposit(
        &mut self,
        account_id: u64,
        currency: &str,
        amount: Decimal,
    ) -> Result<(), SpotLedgerErr> {
        let mut tx = SingleCurrencyTx {
            spot_id: self.advance_spot_id(),
            spot: Some(self.get_spot_or_default(account_id, currency)),
            frozen_receipt: None,
        };
        _ = tx.add_deposit(amount)?;
        _ = self.commit(tx.spot_id, 0, tx.clone(), tx); // Add Deposit是内部操作, 沒有对应bizAction

        Ok(())
    }
}

impl SpotLedgerMatchResultConsumer for SpotLedger {
    // 对taker和maker 依次执行
    fn fill_order(
        &mut self,
        match_result: &FillRecord,
    ) -> Result<Vec<SpotChangeResult>, SpotLedgerErr> {
        let spot_id = self.advance_spot_id();
        let taker_order_id = match_result.taker_order_id.clone();
        let maker_order_id = match_result.maker_order_id.clone();
        let mut taker_base_tx = self.get_spot_and_frozen(
            match_result.taker_account_id,
            &match_result.trade_pair.base,
            &taker_order_id,
        );
        let mut taker_quote_tx = self.get_spot_and_frozen(
            match_result.taker_account_id,
            &match_result.trade_pair.quote,
            &taker_order_id,
        );
        let mut maker_base_tx = self.get_spot_and_frozen(
            match_result.maker_account_id,
            &match_result.trade_pair.base,
            &maker_order_id,
        );
        let mut maker_quote_tx = self.get_spot_and_frozen(
            match_result.maker_account_id,
            &match_result.trade_pair.quote,
            &maker_order_id,
        );

        // checker
        if taker_base_tx.spot.is_none()
            || taker_quote_tx.spot.is_none()
            || maker_base_tx.spot.is_none()
            || maker_quote_tx.spot.is_none()
        {
            return Err(SpotLedgerErr::new(
                err_code::ERR_INPOSSIBLE_STATE,
                "Spot is None in fill_order",
            ));
        }

        let before = [
            taker_base_tx.clone(),
            taker_quote_tx.clone(),
            maker_base_tx.clone(),
            maker_quote_tx.clone(),
        ];

        // do tx.
        match match_result.direction {
            Direction::Buy => {
                // BTC/USD, price=10000, qty=1
                // taker: BTC+1, USD-10000, USD_FROZEN-10000
                // maker: BTC-1，USD+10000, BTC_FROZEN-1
                taker_base_tx.add_deposit(match_result.qty)?;
                taker_quote_tx.sub_deposit(match_result.price * match_result.qty)?;
                if match_result.is_taker_fulfilled {
                    taker_quote_tx.release_all()?;
                } else {
                    taker_quote_tx.release_frozen(match_result.price * match_result.qty)?;
                }
                maker_base_tx.sub_deposit(match_result.qty)?;
                maker_quote_tx.add_deposit(match_result.price * match_result.qty)?;
                if match_result.is_maker_fulfilled {
                    maker_base_tx.release_all()?;
                } else {
                    maker_base_tx.release_frozen(match_result.qty)?;
                }
            }
            Direction::Sell => {
                taker_base_tx.sub_deposit(match_result.qty)?;
                taker_quote_tx.add_deposit(match_result.price * match_result.qty)?;
                if match_result.is_taker_fulfilled {
                    taker_base_tx.release_all()?;
                } else {
                    taker_base_tx.release_frozen(match_result.qty)?;
                }
                maker_base_tx.add_deposit(match_result.qty)?;
                maker_quote_tx.sub_deposit(match_result.price * match_result.qty)?;
                if match_result.is_maker_fulfilled {
                    maker_quote_tx.release_all()?;
                } else {
                    maker_quote_tx.release_frozen(match_result.price * match_result.qty)?;
                }
            }
            _ => Err(SpotLedgerErr::new(
                err_code::ERR_INPOSSIBLE_STATE,
                "Unknown direction in fill_order",
            ))?,
        }

        let after = [taker_base_tx, taker_quote_tx, maker_base_tx, maker_quote_tx];

        // 生成spot_log
        Ok(before
            .into_iter()
            .zip(after.into_iter())
            .map(|(before, after)| self.commit(spot_id, BizAction::FillOrder as i32, before, after))
            .collect())
    }

    fn cancel_order(
        &mut self,
        result: &CancelOrderResult,
    ) -> Result<SpotChangeResult, SpotLedgerErr> {
        if !result.is_cancel_success {
            return Err(SpotLedgerErr::new(
                err_code::ERR_INPOSSIBLE_STATE,
                "Cancel order failed",
            ));
        }

        let currency: &str = match result.direction {
            Direction::Buy => result.trade_pair.quote.as_ref(),
            Direction::Sell => result.trade_pair.base.as_ref(),
            _ => {
                return Err(SpotLedgerErr::new(
                    err_code::ERR_INPOSSIBLE_STATE,
                    "Unknown direction",
                ));
            }
        };
        let spot_id = self.advance_spot_id();
        let mut tx = self.get_spot_and_frozen(result.account_id, currency, &result.order_id);
        if tx.spot.is_none() {
            return Err(SpotLedgerErr::new(
                err_code::ERR_INPOSSIBLE_STATE,
                "Spot is None in cancel_order",
            ));
        }
        let before = tx.clone();
        tx.release_all()?;
        Ok(self.commit(spot_id, BizAction::CancelOrder as i32, before, tx))
    }
}

impl SpotLedgerRPCHandler for SpotLedger {
    fn place_order(
        &mut self,
        order: Order,
        amount: Decimal,
    ) -> Result<SpotChangeResult, SpotLedgerErr> {
        // query: 从self获取最新状态, 构建SingleCurrencyTx
        let spot_id = self.advance_spot_id();
        let currency = match order.direction {
            Direction::Buy => order.trade_pair.quote.as_ref(),
            Direction::Sell => order.trade_pair.base.as_ref(),
            _ => unreachable!(),
        };
        let before = SingleCurrencyTx {
            spot_id: spot_id,
            spot: Some(self.get_spot_or_default(order.account_id, currency)),
            frozen_receipt: None,
        };
        let mut tx = before.clone();
        tx.freeze(&order.order_id, amount)?;
        Ok(self.commit(spot_id, BizAction::PlaceOrder as i32, before, tx))
    }
}

#[derive(Debug, Clone)]
pub struct SpotLedgerErr {
    module: &'static str,
    code: i32,
    msg: &'static str,
}

impl SpotLedgerErr {
    fn new(code: i32, msg: &'static str) -> Self {
        SpotLedgerErr {
            module: "SpotLedger",
            code,
            msg,
        }
    }
}

impl TradeEngineErr for SpotLedgerErr {
    fn code(&self) -> i32 {
        self.code
    }

    fn module(&self) -> &'static str {
        self.module
    }
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
    use crate::testkit as spotkit;
    use crate::testkit::add_balance;
    use rust_decimal::prelude::FromPrimitive;
    use rust_decimal_macros::dec;
    use tte_testkit::testkit;

    #[test]
    fn test_place_order() {
        let mut ledger = SpotLedger::new();
        add_balance(&mut ledger, 1001, "BTC", 1.0);
        add_balance(&mut ledger, 1001, "USDT", 10000.0);
        add_balance(&mut ledger, 1001, "USDT", 2000.0);

        let balance = ledger.get_all_balances(1001);
        assert!(
            balance.get(&"BTC".to_string()).unwrap().deposit == Decimal::from_f64(1.0).unwrap()
        );
        assert!(
            balance.get(&"USDT".to_string()).unwrap().deposit
                == Decimal::from_f64(12000.0).unwrap()
        );
    }

    #[test]

    fn test_fill() {
        // 模拟1001, 1002, 1003两个订单完全成交
        // 1002: 初始状态(BTC_deposit=1, frozen=0), 执行：以10000*0.3价格卖出1个BTC, 以9800*0.5价格卖出0.2个BTC
        // 1001：初始状态(USDT_deposit=10000, frozen=0), 执行以10000*0.5价格买入1个BTC
        let mut ledger = SpotLedger::new();
        spotkit::add_balance(&mut ledger, 1001, "USDT", 10000.0);
        spotkit::add_balance(&mut ledger, 1002, "BTC", 1.0);

        let orders = [
            testkit::new_limit_order("ORD10000001", 1001, Direction::Buy, dec!(10000), dec!(0.5)),
            testkit::new_limit_order("ORD10000002", 1002, Direction::Sell, dec!(10000), dec!(0.3)),
            testkit::new_limit_order("ORD10000003", 1002, Direction::Sell, dec!(9800), dec!(0.2)),
        ];
        for order in orders.iter() {
            let amount = match order.direction {
                Direction::Buy => order.price * order.target_qty,
                Direction::Sell => order.target_qty,
                _ => {
                    panic!()
                }
            };
            tracing::info!("Placing order: {:?}", amount.to_string());
            let _ = ledger.place_order(order.clone(), amount).unwrap();
        }

        let match_records = [
            testkit::fullfill_both(&orders[0], &orders[1], dec!(0.3)),
            testkit::fullfill_both(&orders[0], &orders[2], dec!(0.2)),
        ];

        for match_record in match_records.into_iter() {
            for record in match_record.fill_result.unwrap().results.iter() {
                let _ = ledger.fill_order(record).unwrap();
            }
        }

        let balance_1001 = ledger.get_all_balances(1001);
        let balance_1002 = ledger.get_all_balances(1002);

        println!("Balance 1001: {:?}", balance_1001);
        println!("Balance 1002: {:?}", balance_1002);

        assert!(balance_1001.get(&"USDT".to_string()).unwrap().deposit == dec![5040.0]);
        assert!(balance_1001.get(&"BTC".to_string()).unwrap().deposit == dec![0.5]);
        assert!(balance_1002.get(&"BTC".to_string()).unwrap().deposit == dec![0.5]);

        // frozen必须被完全释放
        assert!(balance_1001.get(&"USDT".to_string()).unwrap().frozen == Decimal::ZERO);
        assert!(balance_1002.get(&"BTC".to_string()).unwrap().frozen == Decimal::ZERO);
    }

    #[test]
    fn test_partially_filled_and_cancel() {
        // 模拟1001下单后撤单
        // 1001：初始状态(USDT_deposit=10000, frozen=0), 执行以10000*0.5价格买入1个BTC
        // 1002--部分成交，成交0.5个BTC，剩余0.5个BTC撤单
        // 操作2：撤单
        let mut ledger = SpotLedger::new();
        add_balance(&mut ledger, 1001, "USDT", 10000.0);
        add_balance(&mut ledger, 1002, "BTC", 1.0);

        let orders = [
            testkit::new_limit_order("ORD10000001", 1001, Direction::Buy, dec!(10000), dec!(0.5)),
            testkit::new_limit_order("ORD10000002", 1002, Direction::Sell, dec!(10000), dec!(0.3)),
        ];
        for order in orders.iter() {
            let amount = match order.direction {
                Direction::Buy => order.price * order.target_qty,
                Direction::Sell => order.target_qty,
                _ => {
                    panic!()
                }
            };
            tracing::info!("Placing order: {:?}", amount.to_string());
            let _ = ledger.place_order(order.clone(), amount).unwrap();
        }

        let match_records = [testkit::fill_buy_limit_order(
            &orders[0],
            &orders[1],
            dec!(0.3),
        )];

        for match_record in match_records.into_iter() {
            for record in match_record.fill_result.unwrap().results.iter() {
                let _ = ledger.fill_order(record).unwrap();
            }
        }

        let balance_1001 = ledger.get_all_balances(1001);
        let balance_1002 = ledger.get_all_balances(1002);

        println!("Balance 1001: {:?}", balance_1001);
        println!("Balance 1002: {:?}", balance_1002);

        assert!(
            balance_1001.get(&"USDT".to_string()).unwrap().deposit
                == Decimal::from_f64(7000.0).unwrap()
        );
        assert!(
            balance_1001.get(&"BTC".to_string()).unwrap().deposit
                == Decimal::from_f64(0.3).unwrap()
        );
        assert!(
            balance_1001.get(&"USDT".to_string()).unwrap().frozen
                == Decimal::from_f64(2000.0).unwrap(),
        );

        assert!(
            ledger
                .cancel_order(
                    &testkit::cancel_buy_limit_order(&orders[0])
                        .cancel_result
                        .unwrap()
                )
                .is_ok()
        );
        let balance_1001_after = ledger.get_all_balances(1001);
        println!("Balance 1001 after cancel: {:?}", balance_1001_after);
        assert!(balance_1001_after.get(&"USDT".to_string()).unwrap().deposit == dec!(7000.0));
        assert!(balance_1001_after.get(&"BTC".to_string()).unwrap().deposit == dec!(0.3));
        assert!(balance_1001_after.get(&"USDT".to_string()).unwrap().frozen == Decimal::ZERO); // 完全释放
    }
}
