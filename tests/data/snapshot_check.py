import json
import os
from decimal import Decimal, getcontext
import sys
from typing import List, Callable, Tuple

sys.path.append(os.path.dirname(__file__))
from checker import *

# 设置 Decimal 的精度以进行精确的金额和数量比较，避免浮点数问题
getcontext().prec = 18

class SnapshotChecker:
    def __init__(self, 
        oms_self_checks: List[Callable[any, List[CheckError]]], # type: ignore
        oms_ob_cross_checks: List[Callable[any, List[CheckError]]], # type: ignore
        ledger_self_checks: List[Callable[any, List[CheckError]]], # type: ignore
    ):
        self.oms_self_checks = oms_self_checks or []
        self.oms_ob_cross_checks = oms_ob_cross_checks or []
        self.ledger_self_checks = ledger_self_checks or []
        
    def stat(self, oms_snapshot: dict):
        """
        统计快照中的活跃订单、终态订单数量。
        """
        active_orders = oms_snapshot.get("active_orders", [])
        final_orders = oms_snapshot.get("final_orders", [])
        num_active_orders, num_final_orders = 0, 0

        # active_orders.<aid>.bid_orders
        for aid_str, aid_orders in active_orders.items():
            bid_orders = aid_orders.get("bid_orders", [])
            ask_orders = aid_orders.get("ask_orders", [])
            num_active_orders += len(bid_orders) + len(ask_orders)
        for account_id, orders in final_orders.items():
            num_final_orders += len(orders)

        print(f"活跃订单数量: {num_active_orders}")
        print(f"终态订单数量: {num_final_orders}")
       

    def main(self, work_dir: str, trade_pairs: List[str]):
        """
        主数据检查脚本。
        """
        print(f"==================================================")
        print(f"🚀 开始数据一致性检查 (交易对: {trade_pairs})")
        print(f"==================================================")
        
        path, oms_snapshot = self.scan_last_oms_snapshot(dir_path=work_dir)
        print(f"✅ 加载 OMS 快照: {path}")
        pairs = [tp.replace("_", "") for tp in trade_pairs] # BASEQUOTE, 用于有效性检查

        # hashmap of (trade_pair, snapshot_data)
        orderbook_snapshots = dict()
        for tp in pairs:
            path, data = self.scan_last_orderbook_snapshot(dir_path=work_dir, trade_pair=tp)
            print(f"✅ 加载订单簿快照: {path}")
            orderbook_snapshots[tp] = data

        if oms_snapshot is None or any(snap is None for snap in orderbook_snapshots.values()):
            print("❌ 错误: 无法加载所有必要的快照文件，检查终止。")
            return

        self.stat(oms_snapshot)

        check_results: List[CheckError] = []

        ## --- OMS 自检 ---
        for check in self.oms_self_checks:
            result = check(oms_snapshot)
            if result:
                check_results.extend(result)
        ## --- 账本自检 ---
        for check in self.ledger_self_checks:
            result = check(oms_snapshot)
            if result:
                check_results.extend(result)
        ## --- OMS-撮合一致性检查 ---
        for check in self.oms_ob_cross_checks:
            result = check(oms_snapshot, orderbook_snapshots)
            if result:
                check_results.extend(result)
        
        # --- 结果汇总 ---
        print("\n==================================================")
        print("📋 检查结果汇总")
        print("==================================================")

        if check_results:
            print(f"❌ 发现 {len(check_results)} 个不一致项:")
            for result in check_results:
                print(result.message)
            print("\n**数据检查失败**")
        else:
            print("✅ 所有数据一致性检查通过!")

        print("==================================================")

    # 扫描形如 dir/oms_snapshot_18_1234.json 的文件, 取最后一个
    def scan_last_oms_snapshot(self, dir_path: str) -> Tuple[str, dict]:
        """
        扫描指定工作目录和日期下的 OMS 快照文件，返回最新的快照文件路径。
        """
        if not os.path.exists(dir_path):
            print(f"❌ 错误: 快照目录不存在: {dir_path}")
            return None

        snapshot_files = [f for f in os.listdir(dir_path) if f.startswith("oms_snapshot_") and f.endswith(".json")]
        if not snapshot_files:
            print(f"❌ 错误: 未找到任何 OMS 快照文件在目录: {dir_path}")
            return None

        # 假设文件名格式为 oms_snapshot_<timestamp>_<id>.json，按 timestamp 排序
        snapshot_files.sort(reverse=True)
        latest_snapshot_file = snapshot_files[0]
        with open(os.path.join(dir_path, latest_snapshot_file), "r") as f:
            snapshot_data = json.load(f)
        return latest_snapshot_file, snapshot_data

    # 扫描形如 dir/orderbook_snapshot_BTCUSDT_13_1765631209005.json, 取最新一个
    def scan_last_orderbook_snapshot(self, dir_path: str, trade_pair: str) -> Tuple[str, dict]:
        """
        扫描指定工作目录、日期和交易对下的订单簿快照文件，返回最新的快照文件路径。
        """
        if not os.path.exists(dir_path):
            print(f"❌ 错误: 快照目录不存在: {dir_path}")
            return None

        snapshot_files = [f for f in os.listdir(dir_path) if f.startswith(f"orderbook_snapshot_{trade_pair}_") and f.endswith(".json")]
        if not snapshot_files:
            print(f"❌ 错误: 未找到任何订单簿快照文件在目录: {dir_path}")
            return None

        # 假设文件名格式为 <trade_pair>_snapshot_<timestamp>_<id>.json，按 timestamp 排序
        snapshot_files.sort(reverse=True)
        latest_snapshot_file = snapshot_files[0]
        with open(os.path.join(dir_path, latest_snapshot_file), "r") as f:
            snapshot_data = json.load(f)
        return latest_snapshot_file, snapshot_data

if __name__ == "__main__":
    # usage:
    # python3 \
    #   ./tests/data/snapshot_check.py --dir=./snapshot

    checker = SnapshotChecker(
        oms_self_checks=[
            check_active_order_state,
            check_trade_id_sequence,
            check_active_order_qty,
            check_final_order_qty,
        ],
        oms_ob_cross_checks=[
            check_order_existence,
            check_matching_qty_consistency,
        ],
        ledger_self_checks=[
            check_frozen_balance,
            check_ledger_sum_eq_0,
        ],
    )

    import argparse
    parser = argparse.ArgumentParser(description="数据一致性检查工具")
    parser.add_argument('--dir', type=str, required=True, help='工作目录路径，包含快照文件')
    opts = parser.parse_args()
    print(f"工作目录: {opts.dir}")

    total_path = os.path.abspath(opts.dir)
    print(f"total_path={total_path}")
    checker.main(work_dir=total_path, trade_pairs=["BTC_USDT", "ETH_USDT"])
