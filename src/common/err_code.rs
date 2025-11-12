//!
//! 错误代码定义
//! 每个domain分配100个号码

pub static ERR_INTERNAL: i32 = 1000000; // OMS内部错误, 一般用于各个组件非预期错误
pub static ERR_INPOSSIBLE_STATE: i32 = 1000001; // 不可能的状态
pub static ERR_INVALID_REQUEST: i32 = 1000002; // 请求参数无效

pub static ERR_OB_ORDER_TYPE_TIF: i32 = 1000001; // 订单类型或时间有效性错误
pub static ERR_OB_ORDER_PRICE_OVERFLOW: i32 = 1000002; // 订单价格超出波动范围
pub static ERR_OB_INVALID_SEQ_ID: i32 = 1000003; // seqID无效

pub static ERR_OB_ORDER_NOT_FOUND: i32 = 100102; // 订单未找到
pub static ERR_OB_ORDER_FILLED: i32 = 1000003; // 订单已经完全成交
pub static ERR_OB_ORDER_CANCELED: i32 = 1000004; // 订单已经被取消

pub static ERR_OMS_PAIR_NOT_TRADING: i32 = 100201; // 交易对不在交易状态
pub static ERR_OMS_PRICE_OUT_OF_RANGE: i32 = 100202; // 价格超出范围
pub static ERR_OMS_QTY_OUT_OF_RANGE: i32 = 100203; // 数量过小
pub static ERR_OMS_DUPLICATE_PLACE: i32 = 100204; // 重复下单
pub static ERR_OMS_PAIR_NOT_FOUND: i32 = 100205; // 交易对未找到
pub static ERR_OMS_ORDER_NOT_FOUND: i32 = 100206; // 订单未找到
pub static ERR_OMS_ACCOUNT_NOT_FOUND: i32 = 100207; // 账户未找到

pub static ERR_LEDGER_INSUFFICIENT_BALANCE: i32 = 100301; // 余额不足
pub static ERR_LEDGER_INVALID_ACCOUNT: i32 = 100302; // 无效账户
pub static ERR_LEDGER_INVALID_FROZEN_ID: i32 = 100303; // 无效冻结ID

pub trait TradeEngineErr {
    fn module(&self) -> &'static str;
    fn code(&self) -> i32;
}
