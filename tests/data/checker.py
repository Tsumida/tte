## --- 活跃订单检查 ---
from decimal import Decimal
from typing import Dict
from typing_extensions import List

epsilon = Decimal('0.00000001')  # 用于金额和数量比较的微小值

# 如果检查失败，则抛出此result，记录详细错误信息，如OrderID，账户ID，状态，金额等
class CheckError:
    def __init__(self, message):
        self.message = message


def check_active_order_state(oms_data: dict) -> List[CheckError]:
    """检查 OMS 活跃订单的状态是否为非终态"""
    print("🔎 检查: 活跃订单状态")
    results = []
    active_orders = {}
    for account_id, data in oms_data.get('active_orders', {}).items():
        active_orders.update(data.get('bid_orders', {}))
        active_orders.update(data.get('ask_orders', {}))

    checked_count = 0 
    passed = True
    for order_id, order_data in active_orders.items():
        checked_count += 1 
        state = order_data.get('current_state')
        if state in ['Filled', 'Cancelled', 'Rejected']:
            results.append(CheckError(
                f"❌ 活跃订单状态错误: 订单 {order_id} 状态为终态: {state}"
            ))
            passed = False
    
    if passed:
        print(f"✅ 活跃订单状态检查通过 (核对订单数: {checked_count})")
    else:
        print(f"⚠️ 活跃订单状态检查未通过 (核对订单数: {checked_count})")
    return results

def check_trade_id_sequence(oms_data: dict) -> List[CheckError]:
    """检查订单的 trade_id 顺序"""
    print("🔎 检查:TradeID > PrevTradeID")
    results = []
    all_orders = []
    for account_id, data in oms_data.get('active_orders', {}).items():
        for order in list(data.get('bid_orders', {}).values()) + list(data.get('ask_orders', {}).values()):
            all_orders.append(order['original'])

    for account_id, orders in oms_data.get('final_orders', {}).items():
        for order in orders:
             all_orders.append(order['original'])
             
    checked_count = 0 
    passed = True
    for order in all_orders:
        checked_count += 1  
        order_id = order.get('order_id')
        trade_id = order.get('trade_id')
        prev_trade_id = order.get('prev_trade_id')
        
        # 增加防御性判断，防止None导致的崩溃 (可选优化)
        if trade_id is None or prev_trade_id is None: 
            continue

        if int(trade_id) <= int(prev_trade_id):
            results.append(CheckError(
                f"❌ Trade ID 顺序错误: 订单 {order_id}, trade_id ({trade_id}) <= prev_trade_id ({prev_trade_id})")
            )
            passed = False
    
    if passed:
        print(f"✅ TradeID 顺序检查通过 (核对订单数: {checked_count})")
    else:
        print(f"⚠️ TradeID 顺序检查未通过 (核对订单数: {checked_count})")
    return results

def check_trade_id_uniqueness(oms_data: dict) -> List[CheckError]:
    """检查订单的 TradeID  唯一性"""
    print("🔎 检查: TradeID 唯一性")
    results = []
    trade_ids = set()
    all_orders = []
    for _, data in oms_data.get('active_orders', {}).items():
        for order in list(data.get('bid_orders', {}).values()) + list(data.get('ask_orders', {}).values()):
            all_orders.append(order['original'])

    for _, orders in oms_data.get('final_orders', {}).items():
        for order in orders:
             all_orders.append(order['original'])
             
    checked_count = 0  
    passed = True
    for order in all_orders:
        checked_count += 1  
        order_id = order.get('order_id')
        trade_id = order.get('trade_id')
        
        if trade_id in trade_ids:
            results.append(CheckError(
                f"❌ Trade ID 唯一性错误: 订单 {order_id}, 重复的 trade_id ({trade_id})")
            )
            passed = False
        else:
            trade_ids.add(trade_id)
    if passed:
        print(f"✅ TradeID 唯一性检查通过 (核对订单数: {checked_count})")
    else:
        print(f"⚠️ TradeID 唯一性检查未通过 (核对订单数: {checked_count})")
    return results

def check_active_order_qty(oms_data: dict) -> List[CheckError]:
    """检查活跃订单的 filled_qty 是否小于 target_qty"""
    print("🔎 检查: 活跃订单数量一致性")
    results = []
    active_orders = {}
    for account_id, data in oms_data.get('active_orders', {}).items():
        active_orders.update(data.get('bid_orders', {}))
        active_orders.update(data.get('ask_orders', {}))

    checked_count = 0 
    passed = True
    for order_id, order_data in active_orders.items():
        checked_count += 1  
        target_qty = Decimal(order_data['original'].get('target_qty', '0'))
        filled_qty = Decimal(order_data.get('filled_qty', '0'))
        
        if filled_qty >= target_qty:
            results.append(CheckError(f"❌ 活跃订单数量错误: 订单 {order_id}, filled_qty ({filled_qty}) >= target_qty ({target_qty})"))
            passed = False
    
    if passed:
        print(f"✅ 活跃订单数量一致性检查通过 (核对订单数: {checked_count})")
    else:
        print(f"⚠️ 活跃订单数量一致性检查未通过 (核对订单数: {checked_count})")
    return results

## --- 终态订单检查 ---

def check_final_order_qty(oms_data: dict) -> List[CheckError]:
    """检查终态订单的 filled_qty 是否小于或等于 target_qty"""
    print("🔎 检查: 终态订单数量一致性")
    results = []
    passed = True
    checked_count = 0  

    for account_id, orders in oms_data.get('final_orders', {}).items():
        for order_data in orders:
            checked_count += 1  
            order_id = order_data['original'].get('order_id')
            target_qty = Decimal(order_data['original'].get('target_qty', '0'))
            filled_qty = Decimal(order_data.get('filled_qty', '0'))
            
            if filled_qty > target_qty:
                results.append(CheckError(
                    f"❌ 终态订单数量错误: 订单 {order_id}, filled_qty ({filled_qty}) > target_qty ({target_qty})")
                )
                passed = False
                
    if passed:
        print(f"✅ 终态订单数量一致性检查通过 (核对订单数: {checked_count})")
    else:
        print(f"⚠️ 终态订单数量一致性检查未通过 (核对订单数: {checked_count})")
    return results

## --- OMS-撮合一致性检查 ---

def check_order_existence(oms_data: dict, matching_data: Dict[str, dict]) -> List[CheckError]:
    """检查 OMS 活跃订单是否在对应交易对的撮合快照中存在，反之亦然"""
    print("🔎 检查: 订单簿存在性")
    results = []
    
    # 1. 提取 OMS 中的所有活跃订单 ID
    oms_active_ids_by_tp = {} 
    oms_active_ids = set()
    for account_id, data in oms_data.get('active_orders', {}).items():
        for order_data in list(data.get('bid_orders', {}).values()) + list(data.get('ask_orders', {}).values()):
            # ... (原有逻辑省略) ...
            original = order_data.get('original', {})
            order_id = original.get('order_id')
            if not order_id: continue
            tp_info = original.get('trade_pair') or {}
            base = tp_info.get('base')
            quote = tp_info.get('quote')
            if not base or not quote: continue
            tp_key = f"{base}{quote}"
            oms_active_ids.add(order_id)
            if tp_key not in oms_active_ids_by_tp:
                oms_active_ids_by_tp[tp_key] = set()
            oms_active_ids_by_tp[tp_key].add(order_id)
    
    # 2. 提取撮合中的所有订单 ID
    matching_ids_by_tp = {} 
    for tp_key, snapshot in matching_data.items():
        ids = set()
        # ... (原有逻辑省略) ...
        for order_data in snapshot.get('bid_orders', []):
            order = order_data.get('order', {})
            order_id = order.get('order_id')
            if order_id: ids.add(order_id)
        for order_data in snapshot.get('ask_orders', []):
            order = order_data.get('order', {})
            order_id = order.get('order_id')
            if order_id: ids.add(order_id)
        matching_ids_by_tp[tp_key] = ids
    
    matching_ids = set()
    for ids in matching_ids_by_tp.values():
        matching_ids.update(ids)
    
    # 3. 对比
    passed = True
    checked_oms_count = 0
    checked_me_count = 0

    # 3a. OMS -> ME
    for tp_key, oms_ids in oms_active_ids_by_tp.items():
        snapshot_ids = matching_ids_by_tp.get(tp_key)
        for order_id in oms_ids:
            checked_oms_count += 1 
            if not snapshot_ids or order_id not in snapshot_ids:
                results.append(CheckError(f"❌ OMS与ME数据不一致: OMS活跃订单 {order_id} 不在撮合买卖盘"))
                passed = False
        
    # 3b. ME -> OMS
    for tp_key, snapshot_ids in matching_ids_by_tp.items():
        oms_ids = oms_active_ids_by_tp.get(tp_key, set())
        for order_id in snapshot_ids:
            checked_me_count += 1 
            if order_id not in oms_ids:
                results.append(CheckError(f"❌ 订单簿存在性错误: 撮合快照订单 {order_id} 丢失于 OMS 活跃订单"))
                passed = False
        
    if passed:
        print(f"✅ 订单簿存在性检查通过 (OMS侧: {checked_oms_count}, ME侧: {checked_me_count})")
    else:
        print(f"⚠️ 订单簿存在性检查未通过 (OMS侧: {checked_oms_count}, ME侧: {checked_me_count})")
    return results

def check_matching_qty_consistency(oms_data: dict, matching_data: Dict[str, dict]) -> List[CheckError]:
    """检查 OMS 活跃订单的剩余数量是否与撮合订单的 remain_qty 一致"""
    print("🔎 检查: 撮合数量一致性")
    results = []

    oms_remain_qty_by_tp = {}
    
    for account_id, data in oms_data.get('active_orders', {}).items():
        for order_data in list(data.get('bid_orders', {}).values()) + list(data.get('ask_orders', {}).values()):
            original = order_data.get('original', {})
            order_id = original.get('order_id')
            if not order_id: continue
            tp_info = original.get('trade_pair') or {}
            base = tp_info.get('base')
            quote = tp_info.get('quote')
            if not base or not quote: continue
            tp_key = f"{base}{quote}"
            target_qty = Decimal(original.get('target_qty', '0'))
            filled_qty = Decimal(order_data.get('filled_qty', '0'))
            remain_qty = target_qty - filled_qty
            if tp_key not in oms_remain_qty_by_tp:
                oms_remain_qty_by_tp[tp_key] = {}
            oms_remain_qty_by_tp[tp_key][order_id] = remain_qty

    matching_remain_qty_by_tp = {}
    for tp_key, snapshot in matching_data.items():
        per_tp = {}
        for order_data in snapshot.get('bid_orders', []) + snapshot.get('ask_orders', []):
            order = order_data.get('order', {})
            order_id = order.get('order_id')
            if not order_id: continue
            remain_qty = Decimal(order_data.get('qty_info', {}).get('remain_qty', '0'))
            per_tp[order_id] = remain_qty
        matching_remain_qty_by_tp[tp_key] = per_tp

    # 3. 对比
    passed = True
    checked_count = 0 
    
    for tp_key, oms_orders in oms_remain_qty_by_tp.items():
        matching_orders = matching_remain_qty_by_tp.get(tp_key, {})
        for order_id, oms_qty in oms_orders.items():
            matching_qty = matching_orders.get(order_id)
            if matching_qty is None:
                continue

            checked_count += 1
            
            if abs(oms_qty - matching_qty) > epsilon:
                results.append(CheckError(
                    f"❌ 撮合数量不一致: 订单 {order_id}, OMS 剩余数量: {oms_qty}, 撮合剩余数量: {matching_qty}"
                ))
                passed = False

    if passed:
        print(f"✅ 撮合数量一致性检查通过 (对比订单数: {checked_count})")
    else:
        print(f"⚠️ 撮合数量一致性检查未通过 (对比订单数: {checked_count})")
    return results


## --- OMS-账本一致性检查 ---
def check_frozen_balance(oms_data: dict) -> List[CheckError]:
    """检查 ledger.spots 中 frozen 余额是否与 order_frozen_receipts 汇总一致"""
    print("🔎 检查: 冻结余额一致性")
    results = []
    
    # 1. 计算 (逻辑不变...)
    receipt_frozen_sum = {}
    for receipt in oms_data.get('ledger', {}).get('order_frozen_receipts', {}).values():
        account_id = receipt['account_id']
        currency = receipt['currency']
        remain_frozen = Decimal(receipt.get('remain_frozen', '0'))
        key = (account_id, currency)
        receipt_frozen_sum[key] = receipt_frozen_sum.get(key, Decimal('0')) + remain_frozen

    # 2. 提取 (逻辑不变...)
    ledger_frozen = {}
    for account_id, spot_data in oms_data.get('ledger', {}).get('spots', {}).items():
        for currency, balance_data in spot_data.items():
            frozen = Decimal(balance_data.get('frozen', '0'))
            key = (int(account_id), currency)
            ledger_frozen[key] = frozen

    # 3. 对比
    passed = True
    all_keys = set(receipt_frozen_sum.keys()) | set(ledger_frozen.keys())
    checked_count = 0 
    
    for key in all_keys:
        checked_count += 1 
        account_id, currency = key
        receipt_sum = receipt_frozen_sum.get(key, Decimal('0'))
        ledger_bal = ledger_frozen.get(key, Decimal('0'))
        
        if abs(receipt_sum - ledger_bal) > epsilon:
            alert = CheckError(message=f"❌ 冻结余额不一致: 账户 {account_id}, 币种 {currency}...")
            results.append(alert)
            passed = False
            
    if passed:
        print(f"✅ 冻结余额一致性检查通过 (核对账户-币种对: {checked_count})")
    else:
        print(f"⚠️ 冻结余额一致性检查未通过 (核对账户-币种对: {checked_count})")
    return results

def check_ledger_sum_eq_0(oms_data: dict) -> List[CheckError]:
    """检查 ledger.spots 中借贷平衡"""
    print("🔎 检查: 账本余额归零")
    results = []
    
    passed = True
    checked_count = 0 
    
    for account_id, spot_data in oms_data.get('ledger', {}).get('spots', {}).items():
        currency_sums = {}
        for currency, balance_data in spot_data.items():
            balance = Decimal(balance_data.get('balance', '0'))
            currency_sums[currency] = currency_sums.get(currency, Decimal('0')) + balance
        
        for currency, total_balance in currency_sums.items():
            checked_count += 1 
            if abs(total_balance) > epsilon:
                results.append(CheckError(
                    f"❌ 账本余额不归零: 账户 {account_id}, 币种 {currency}, 总余额: {total_balance}"
                ))
                passed = False
    
    if passed:
        print(f"✅ 账本余额归零检查通过 (核对账户-币种对: {checked_count})")
    else:
        print(f"⚠️ 账本余额归零检查未通过 (核对账户-币种对: {checked_count})")
    return results